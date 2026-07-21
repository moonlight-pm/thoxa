- Read .claude/context.md
- Be inquisitive, ask clarifying questions, involved the user as much as possible in design choices.
- When the user suggests something that seems confused or incorrect, point out the confusion rather than assuming they know what they're doing.
- Do what has been asked; nothing more, nothing less.
- Do not be overly enthusiastic.  Do not tell me I am exactly right without knowing whether I am or not.
- Never create files unless they're absolutely necessary for achieving your goal.
- Always ask the user before installing any new packages, to verify that is what they want. Present alternatives.
- Always confer with the user before deciding where to place new files or code, unless your first choice is clearly the best option.
- Write unit tests for everything
- Write end-to-end tests for completed features (.thx files that go in examples/tests), and update examples/tests/run_tests.sh
- Use Makefile commands instead of raw commands (e.g., use `make build` instead of `cargo build`, `make test` instead of `cargo test`, etc.)
- When compiling Thoxa code, use the Makefile or add appropriate make targets rather than calling the compiler directly

## Documentation Structure Rules

### /docs - Completed Features Documentation
- Documents features as they are currently implemented
- Should accurately reflect the actual state of the codebase
- Features marked as "Planned" or "Future" should be clearly labeled
- This is the "ideal" or "completed feature" documentation

### /plans - Implementation Plans
- Contains plans for incomplete features or groups of features  
- Should only show what remains to be done, not completed work
- Completed work should be removed from plans
- Active development plans should be kept up-to-date
- The /plans folder is for maintaining plans on individual features or components of the app

## Testing Guidelines
- All enabled tests in examples/tests/*.thx currently pass
- Use test files to verify feature completion status when unsure
- Check the codebase for implementation details

## Implementation Status
See PROGRESS.md in the project root for detailed feature completion status
