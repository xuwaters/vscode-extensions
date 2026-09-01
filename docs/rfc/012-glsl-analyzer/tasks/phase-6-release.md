# Phase 6 — Polish & release

**Goal:** ship it. **Needs:** Phase 5.

**Exit criterion:** VSIX packaged from a clean build; user/dev docs updated;
measurements and notices in place.

| ID | Task | Test | Status | Notes |
| --- | --- | --- | --- | --- |
| P6-01 | `examples/` refreshed to exercise the new analyzer (an ES 300 shader, an OpenGL combined-sampler shader, a Vulkan 450 shader — each clean under our diagnostics); README.md user documentation for GLSL analysis (README = product manual, per repo convention). | examples pass the no-error gate | ☐ | |
| P6-02 | CONTRIBUTING.md dev docs: crate map, how to regenerate the spec (`temp/docs.gl` + one command), how corpus tests skip, budgets and where they are asserted. | n/a | ☐ | |
| P6-03 | Third-party notices finalised (docs.gl/Khronos attribution from P1-10; confirm no other new obligations). | n/a | ☐ | |
| P6-04 | Final measurements sweep into [research/measurements.md](../research/measurements.md): corpus pass rates, diagnostic counts, budgets, before/after feature comparison on the examples. | recorded numbers | ☐ | |
| P6-05 | Version bump + CHANGELOG + `pnpm`-side wiring checked (`.vscodeignore` still correct) + VSIX packaged. | packaged VSIX opens with features live | ☐ | |
| P6-06 | RFC closeout: board updated to final, open questions all closed, deferred items (§10) restated as a short list for a future RFC. | n/a | ☐ | |
