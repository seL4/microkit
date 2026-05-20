# Making Microkit releases

There's a couple different parts to doing a Microkit release,
some of which are largely automated and some that cannot be.

The general order for making a release is:
1. Tagging a new release and writing release notes.
2. Producing SDK artifacts.
3. Making the GitHub release.
4. Updating documentation and projects that use Microkit.
5. (optional) wait for bugs and make a bug-fix release.

## Step 1 - tagging and release notes

Microkit uses semantic versioning and so the version should be bumped appropriately
whether or not a breaking change has been made.

Make a tag with `git tag <version>`.

In the past, the process for writing release notes is to do `git log <previous release>..<next release>`
and go through each commit and if it's relevant for users then write something about it.

Piping the `git log` output into a file can be useful to keep track of what commits you have left to
write up.

It is a good idea to provide some context on why a feature was added or a change was made so users
know *why* it exists.

Lastly, any breaking changes should be thoroughly explained and have an example on how to upgrade,
see the [bottom of 2.0.0's release notes](https://docs.sel4.systems/releases/microkit/2.0.0)
for an example of this.

The release notes should be added to `CHANGES.md` inside Microkit, as well as the seL4 documentation
website.

## Step 2 - SDK artifacts

The SDK builds are automated by the CI runs. Currently, the artifacts are produced by the
self-hosted Mac Mini which builds the SDK once and uses the `--release-packaging` flag to cross-compile
the tool for each target and then make tarballs for each host target. This is very important since it
ensures that the SDKs have no diff between them (e.g between Linux x86-64 and macOS ARM64) except for
the tool itself since it is host dependent.

However, there are two steps that aren't automated which is GPG signing and macOS binary code-signing.

TODO: add scripts for doing signing
TODO: add guide for dealing with macos signing non-sense

## Step 3 - GitHub release

The GitHub release is pretty trivial, just follow what the past release does as a template.
This is a manual process right now but could be automated using the GitHub CLI.

The SDKs (and associated PGP signature) must be uploaded as part of the GitHub release.

## Step 4 - updating associated projects

In addition to adding the release notes to the seL4 documentation website for each release you should:
1. Look at the Microkit pages on the docs website and see if any need updating.
   The most relevant ones to check for each release is [roadmap](https://docs.sel4.systems/projects/microkit/roadmap.html)
   and [supported platforms](https://docs.sel4.systems/projects/microkit/platforms.html).
2. Update the [Microkit tutorial](https://github.com/au-ts/microkit_tutorial) to use the new SDK.
   The repository has instructions for how to maintain it.

## Step 5 - making bug-fix releases

Maybe you messed up and there are bugs that affects users. In the past after waiting a week or two
we have made bug-fix releases. Bug-fix releases are more worthwhile compared to other projects
such as seL4 itself because users primarily rely on the binary SDK releases, so keep that in mind.
