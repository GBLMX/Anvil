# Local CI with act and Docker

Status: current. This runs the GitHub Actions workflows through
[act](https://github.com/nektos/act) and Docker.

## Prerequisites

```text
act      0.2.98 or newer
docker   daemon reachable by the current user
git
```

The repository-level `.actrc` maps `ubuntu-latest` to:

```text
ghcr.io/catthehacker/ubuntu:act-latest
```

Pull the image once:

```bash
docker pull ghcr.io/catthehacker/ubuntu:act-latest
```

The image is large. The first pull may take several minutes.

## Commands

List the workflows and jobs act can see:

```bash
act -l
```

Run the documentation job:

```bash
act -j docs
```

Run the jobs used by the default `ci.yml` workflow:

```bash
act -j rust
act -j go
act -j docs
act -j control-smoke
act -j release-lifecycle
act -j release-layout
act -j official-plugin-package
```

For pull-request-only jobs, select the event explicitly:

```bash
act pull_request -j semver
act pull_request -j bench-regression
act pull_request -j bindings
```

Validate workflow syntax without starting a container:

```bash
act -n -j docs
```

## Limitations

- The image is a local approximation of `ubuntu-latest`, not an exact GitHub
  hosted runner.
- Network access, Docker-in-Docker, GitHub tokens, release signing secrets,
  attestations, and hosted-runner-only services are not reproduced.
- `release.yml` and `release-publish.yml` require production signing and
  GitHub-specific permissions; do not use them as a local release path.
- `bench-regression` depends on `origin/main` and a clean Git worktree; run it
  only when those local preconditions are satisfied.
- A local pass does not replace CI. It is a fast feedback loop for docs, Rust,
  Go, and control-surface jobs.

## Cleanup

Act uses Docker containers and the action cache. To remove stopped containers
created by act:

```bash
docker container prune
```

To remove the large runner image:

```bash
docker image rm ghcr.io/catthehacker/ubuntu:act-latest
```
