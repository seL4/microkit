#!/usr/bin/env bash

# Copyright 2026, UNSW
# SPDX-License-Identifier: BSD-2-Clause

set -e

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

if [ "$#" -ne 3 ]; then
    echo >&2 "Usage: $0 <microkit-sdk> <seL4/ci-actions checkout> <build-folder>"
    exit 1
fi

export MICROKIT_SDK=$(readlink -f "$1")
export CI_ACTIONS=$(readlink -f "$2")
export GITHUB_WORKSPACE=$(readlink -f "$3")

run_ci_actions_steps() {
    export INPUT_ACTION_NAME="$1"
    action_script="${2:-steps.sh}"

    export SCRIPTS="${CI_ACTIONS}/scripts"
    export PATH="${SCRIPTS}:${PATH}"

    action_dir="${CI_ACTIONS}/${INPUT_ACTION_NAME}"

    "${action_dir}/${action_script}"
}

# secrets
export HW_SSH=$(cat ~/.ssh/id_ed25519)
# pseudo-sandbox
export HOME="${GITHUB_WORKSPACE}"

# used for the job key in machine queue
export GITHUB_REPOSITORY="microkit"
export GITHUB_WORKFLOW="locally"
export GITHUB_RUN_ID="local"
export GITHUB_JOB="local"

# Make sudo a no-op
sudo_tmpdir=$(mktemp -d)
printf '#!/usr/bin/env bash\necho >&2 ignoring sudo "$@"\n' > "${sudo_tmpdir}/sudo"
chmod +x "${sudo_tmpdir}/sudo"
export PATH="${sudo_tmpdir}:${PATH}"

export GITHUB_OUTPUT=$(mktemp)
export GITHUB_ENV=$(mktemp)

mkdir -p "${GITHUB_WORKSPACE}"

# Always create a log file in the build folder.
# use -i so that tee always exits after this script
mkdir -p "${GITHUB_WORKSPACE}/logs"
LOGFILE="${GITHUB_WORKSPACE}/logs/local_run_$(date '+%Y-%m-%d-%H').txt"
echo 2>&1 "Emitting logs to ${LOGFILE}"
exec > >(tee -i "${LOGFILE}") 2>&1

unset PYTHONPATH
python3 -m venv "${GITHUB_WORKSPACE}/venv"
. "${GITHUB_WORKSPACE}/venv/bin/activate"

# don't create __pycache__ folders
export PYTHONDONTWRITEBYTECODE=1

# Pretend microkit is installed here.
mkdir -p "${GITHUB_WORKSPACE}/microkit"
ln -sf "${SCRIPT_DIR}/../build_sdk.py" "${GITHUB_WORKSPACE}/microkit/build_sdk.py"
ln -sf "${SCRIPT_DIR}/../VERSION" "${GITHUB_WORKSPACE}/microkit/VERSION"

cd "${GITHUB_WORKSPACE}"

run_ci_actions_steps "microkit-hw-matrix"

export TEST_CASES=$(cat "${GITHUB_OUTPUT}" | grep "test_cases" | cut -d "=" -f 2)

run_ci_actions_steps "microkit-hw-build"

# Note: This rarely works, because non-interactive bash assumes wait-for-coordinated
# exit and most of our python scripts do not follow this protocol.
trap_handler() {
    really_die() {
        # Implement Wait-and-Cooperative-Exit protocol
        trap - SIGINT
        kill -s SIGINT $$
    }

    echo >&2 "Handling SIGINT signal"
    trap 'really_die' SIGINT

    run_ci_actions_steps "microkit-hw-run" "post-steps.sh"

    really_die
}

trap 'trap_handler' SIGINT

run_ci_actions_steps "microkit-hw-run"
