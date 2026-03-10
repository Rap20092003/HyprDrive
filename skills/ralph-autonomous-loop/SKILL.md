---
name: ralph-autonomous-loop
description: Autonomous AI agent loop that implements PRD user stories one-by-one until all pass — fresh context per iteration, memory via git + progress.txt + prd.json
---

# Ralph — Autonomous Agent Loop

Ralph is an autonomous AI agent loop (based on [Geoffrey Huntley's Ralph pattern](https://ghuntley.com/ralph/)) that runs an AI coding tool repeatedly until all PRD items are complete. Each iteration is a fresh instance with clean context. Memory persists via git history, `progress.txt`, and `prd.json`.

**Source**: [snarktank/ralph](https://github.com/snarktank/ralph)

---

## Workflow

### 1. Create a PRD
Use the `ralph-prd-generator` skill to generate a detailed requirements document.

### 2. Convert PRD to Ralph format
Use the `ralph-prd-converter` skill to convert the markdown PRD into `prd.json`.

### 3. Run Ralph Loop
Ralph will:
1. Create a feature branch (from PRD `branchName`)
2. Pick the highest priority story where `passes: false`
3. Implement that single story
4. Run quality checks (typecheck, tests)
5. Commit if checks pass
6. Update `prd.json` to mark story as `passes: true`
7. Append learnings to `progress.txt`
8. Repeat until all stories pass or max iterations reached

---

## Critical Concepts

### Each Iteration = Fresh Context
Each iteration spawns a new AI instance with clean context. The only memory between iterations is:
- **Git history** (commits from previous iterations)
- **progress.txt** (learnings and context)
- **prd.json** (which stories are done)

### Small Tasks
Each PRD item should be small enough to complete in one context window. If a task is too big, the LLM runs out of context before finishing and produces poor code.

### AGENTS.md Updates Are Critical
After each iteration, update relevant AGENTS.md files with learnings. Future iterations and developers benefit from discovered patterns, gotchas, and conventions.

Examples of what to add:
- Patterns discovered ("this codebase uses X for Y")
- Gotchas ("do not forget to update Z when changing W")
- Useful context ("the settings panel is in component X")

### Feedback Loops
Ralph only works if there are feedback loops:
- **Typecheck** catches type errors
- **Tests** verify behavior
- **CI must stay green** (broken code compounds across iterations)

### Browser Verification for UI Stories
Frontend stories must include "Verify in browser" in acceptance criteria.

### Stop Condition
When all stories have `passes: true`, Ralph outputs `<promise>COMPLETE</promise>` and the loop exits.

---

## Key Files

| File | Purpose |
|------|---------|
| `ralph.sh` | Main loop script (`--tool amp` or `--tool claude`) |
| `prompt.md` | Prompt template for Amp |
| `CLAUDE.md` | Prompt template for Claude Code |
| `prd.json` | Stories with `passes` tracking |
| `progress.txt` | Cross-iteration memory |
| `skills/prd/` | PRD Generator skill |
| `skills/ralph/` | PRD Converter skill |

---

## Debugging

```bash
# See which stories are done
cat prd.json | jq '.userStories[] | {id, title, passes}'

# See learnings from previous iterations
cat progress.txt

# Check git history
git log --oneline -10
```

---

## Archiving

Ralph automatically archives previous runs when you start a new feature (different `branchName`). Archives are saved to `archive/YYYY-MM-DD-feature-name/`.
