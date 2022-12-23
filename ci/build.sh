#!/bin/sh

# This script is used to build the seL4 Core Platform SDK as part of the CI
# process. The intention is that it can also easily be used to reproduce the
# CI artefacts locally.

python3.9 -m venv pyenv
./pyenv/bin/pip install --upgrade pip setuptools wheel
./pyenv/bin/pip install -r requirements.txt

if [ -d seL4 ]; then
	echo "seL4 directory already exists, please remove and run the script again"
	exit 1
fi

git clone https://github.com/Ivan-Velickovic/seL4.git --branch sel4cp-dev
./pyenv/bin/python build_sdk.py --sel4=seL4

