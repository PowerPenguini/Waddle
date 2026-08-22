# Issue tracker: Local Markdown

Issues and specs for this repo live as Markdown files in `.scratch/`.

## Conventions

- One feature per directory: `.scratch/<feature-slug>/`
- The spec is `.scratch/<feature-slug>/spec.md`
- Implementation issues are stored separately in `.scratch/<feature-slug>/issues/`
- Each implementation issue uses `<NN>-<slug>.md`, starting at `01`
- Never combine all implementation tickets into one file
- Record triage state in a `Status:` line near the top of each issue
- Append discussion under a `## Comments` heading

## Publishing

When a skill says to publish a spec or issue to the tracker, create the corresponding file under `.scratch/<feature-slug>/`.

When a skill requests a relevant ticket, read the supplied local path.

## Wayfinding

- Store the feature map in `.scratch/<feature-slug>/map.md`
- Store child tickets in `.scratch/<feature-slug>/issues/`
- Record ticket type in a `Type:` line
- Record blocking tickets in a `Blocked by:` line
- A ticket is available when it is open, unblocked, and unclaimed
- Claim a ticket by setting `Status: claimed`
- Resolve it by adding an `## Answer` section and setting `Status: resolved`
