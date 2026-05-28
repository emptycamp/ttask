## Network Environment

You are operating in a **network-isolated environment**.

**If any network request fails** (e.g., cannot reach a public URL, IP address, or external API):
1. **Stop immediately** — do not retry or attempt workarounds
2. **Report the failure** to the user with the exact URL/address that failed
3. **Ask the user to resolve the connectivity issue** before continuing

## Git Staging

**Never modify the Git index.** This means:
- Do not run `git add`, `git reset`, `git restore --staged`, or any other command that stages or unstages files
- The user manages all staging manually
- You may still read Git state (e.g., `git status`, `git diff`)

## Code quality

Always check project-conventions skill before making any changes!
Ensure `cargo fmt --check` passes
Ensure `cargo build` does not contain any warnings or suggestions
