## What

<!-- One to three sentences describing the change. -->

Refs: <!-- issue, if any -->

## Why

<!-- The problem or requirement this addresses. -->

## How

<!-- Key decisions and trade-offs; anything reviewers should look at closely. -->

## Verification

| Check | Command | Result |
|---|---|---|
| Format | `scripts/check.sh fmt` | <!-- Passed / Failed / Not run (reason) --> |
| Lint | `scripts/check.sh lint` | |
| Tests | `scripts/check.sh test` | <!-- counts --> |
| Build | `scripts/check.sh build` | |
| Dependencies | `scripts/check.sh deny` | |
| Secrets | `scripts/check.sh secrets` | |
| Manual testing | <!-- what you tried, in which terminal/OS --> | |

## Risk and Impact

- **Change risk:** <!-- Low / Medium / High -->
- **Breaking changes:** <!-- None, or describe (CLI options, key bindings, output file names) -->
- **Dependencies:** <!-- None, or added/updated/removed with a reason -->

## Checklist

- [ ] Self-reviewed the full diff
- [ ] Tests added or updated; bug fixes have a regression test
- [ ] Documentation updated (README, CONTRIBUTING, docs/, CHANGELOG) where needed
- [ ] No secrets, personal data, media files, debug code, or unrelated changes

## Known Limitations and Follow-Up

<!-- "None" if none. -->
