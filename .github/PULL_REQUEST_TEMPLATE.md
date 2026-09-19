## Summary

<!-- What does this change and why? Link any related issue. -->

## Test plan

<!-- How did you verify it? Which suites did you run?
     e.g. cargo test, vitest, playwright, manual check on Windows. -->

## Checklist

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` passes
- [ ] `npx vitest run` passes
- [ ] `npx playwright test` passes (if UI behavior changed)
- [ ] `cargo fmt` + `cargo clippy` clean, `npx tsc --noEmit` clean
- [ ] Commit messages follow the conventional-commit style (see CONTRIBUTING.md)
- [ ] UI changes follow DESIGN.md and include a screenshot
- [ ] CHANGELOG.md updated (user-facing changes)
