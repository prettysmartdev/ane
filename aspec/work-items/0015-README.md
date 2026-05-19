# Work Item 0015: Address Agent Usage Feedback

**Status**: Documentation Complete | Ready for Implementation

---

## Documentation Overview

This work item includes three complementary documents designed for different audiences and use cases:

### 1. **[0015-address-agent-usage-feedback.md](0015-address-agent-usage-feedback.md)** — Specification
The authoritative specification document. Contains:
- **Six bug findings** that surfaced in `ane-findings.md` during agent usage
- **User stories** explaining why each fix matters to code agents
- **Implementation details** with exact file locations, functions, and code changes
- **Edge case considerations** documenting interactions between fixes
- **Test considerations** listing all required tests for regression prevention

**Use this when**: You need to understand WHAT needs to be fixed and WHY.

---

### 2. **[0015-IMPLEMENTATION.md](0015-IMPLEMENTATION.md)** — Step-by-Step Guide
A detailed developer guide walking through each fix in sequence. Contains:
- **Implementation order** with dependency analysis
- **Problem statement + root cause** for each fix (why the current code is wrong)
- **Step-by-step implementation instructions** with code snippets
- **Unit test examples** ready to adapt and use
- **Edge case deep-dives** explaining corner cases and why fixes handle them
- **Integration testing guidance** validating multi-fix interactions
- **Completion checklist** for tracking progress
- **Common pitfalls** to avoid during implementation
- **Helpful ane commands** for exploring and editing code

**Use this when**: You're implementing the fixes and need detailed guidance on HOW to do it.

**Recommended approach**: Read Fixes 1–3 in sequence (independent), then Fix 5 (depends on understanding Fix 2), then Fix 4 (documentation), then integration tests.

---

### 3. **[0015-QUICK-REFERENCE.md](0015-QUICK-REFERENCE.md)** — At-a-Glance Summary
A compact reference guide for quick lookups during implementation. Contains:
- **Fix summary table** showing file, function, and impact for each fix
- **Phase-based implementation checklist** with concrete test names
- **Code snippets** for each fix (ready to copy)
- **Key insights** capturing the essence of each fix
- **Testing strategy** by phase
- **Dependencies & interactions** diagram
- **Common issues & solutions** troubleshooting table
- **Helpful ane commands** for file exploration

**Use this when**: You need a quick reference while coding or debugging.

---

## File Organization

```
0015-address-agent-usage-feedback.md     ← Original specification
0015-IMPLEMENTATION.md                   ← Detailed implementation guide (600+ lines)
0015-QUICK-REFERENCE.md                  ← At-a-glance checklist (200+ lines)
0015-README.md                           ← This file (index)
```

---

## The Five Fixes at a Glance

| # | Problem | File | Impact | Code Change |
|---|---------|------|--------|-------------|
| 1 | Buffer-scope chords require dummy `target:1` | `chord.rs` | Allow `yebs` without args | Add `Scope::Buffer` exemption to guard |
| 2 | `aals(target:N)` appends at EOF, not line N | `resolver.rs` | Fix line-end append position | Use `TextRange::point()` for After/Before Self_ |
| 3 | `cifc` merges opening `{` and closing `}` with replacement | `resolver.rs` | Preserve delimiter lines | Advance/retreat past newlines in `find_brace_range` |
| 4 | Agents confuse Name and Definition components | `ane-skill.md`, `core.rs` | Clarify component choice | Add component guide to skill + update tool examples |
| 5 | `aebs(value:"text")` concatenates instead of inserting on new line | `patcher.rs` | Auto-prefix newline at line-end | Check line-end, prefix `\n` if needed |

---

## Implementation Timeline

**Recommended**: 4–6 hours for experienced developer

- **Phase 1** (1–2 hours): Fixes 1–3 (independent, test each one)
- **Phase 2** (1 hour): Fix 5 (depends on Fix 2 understanding)
- **Phase 3** (30 min): Fix 4 (documentation, word-count verification)
- **Phase 4** (1 hour): Integration tests + full regression suite

---

## Key Points for Success

1. **Read the implementation guide before coding**: It explains WHY each fix is needed, not just WHAT to change.

2. **Test each fix in isolation**: The full test suite (`cargo test`) should pass after each fix before moving to the next.

3. **Watch for symmetry**: Fix 2 affects both `Positional::After` and `Positional::Before` — update both.

4. **Understand component semantics**: Fix 4 is documentation, but understanding it helps explain Fixes 1–3 and 5.

5. **Verify edge cases**: Each fix has documented edge cases (single-line functions, empty buffers, etc.). Read them; test them.

6. **Use ane for exploration**: The project philosophy is to dogfood `ane`. The implementation guide includes helpful `ane exec` commands for reading and editing files.

---

## References

- **Project CLAUDE.md**: [/workspace/CLAUDE.md](/workspace/CLAUDE.md) — Architecture, build, test guidelines
- **Chord System Docs**: [/workspace/docs/01-chord-system.md](/workspace/docs/01-chord-system.md) — Grammar and semantics
- **Architecture Overview**: [/workspace/docs/07-architecture-overview.md](/workspace/docs/07-architecture-overview.md) — Layer 0/1/2 design
- **ane-findings.md**: [/workspace/ane-findings.md](/workspace/ane-findings.md) — Original bug reports from agent usage

---

## Next Steps

1. **Start with**: Read the spec ([0015-address-agent-usage-feedback.md](0015-address-agent-usage-feedback.md)) to understand the complete scope.

2. **Then follow**: [0015-IMPLEMENTATION.md](0015-IMPLEMENTATION.md) step-by-step, completing each fix in order.

3. **Use for reference**: [0015-QUICK-REFERENCE.md](0015-QUICK-REFERENCE.md) when you need quick lookups or checklists.

4. **Verify**: Run `cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt --check` after all fixes are complete.

---

## Questions?

- **What is this fix for?** → See [0015-address-agent-usage-feedback.md](0015-address-agent-usage-feedback.md)
- **How do I implement it?** → See [0015-IMPLEMENTATION.md](0015-IMPLEMENTATION.md)
- **What's the quick summary?** → See [0015-QUICK-REFERENCE.md](0015-QUICK-REFERENCE.md)
- **How do I explore/edit files?** → See the "Helpful ane Commands" section in any document

---

## Status

- ✅ Specification document: Complete
- ✅ Implementation guide: Complete (600+ lines, step-by-step)
- ✅ Quick reference: Complete (200+ lines, checklist + snippets)
- ✅ Index/README: Complete (this file)
- ⏳ Implementation: Ready to begin
- ⏳ Tests: Ready to add
- ⏳ Code review: Pending implementation
