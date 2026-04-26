# ui

Entry points. Each subdirectory is one way to run hub.

**Subdirectories:**
- `cli/` — command-line interface; `hub` binary
- `tui/` — terminal UI; `hub-tui` binary

**Rules:**
- Each `main.rs` bootstraps config, wires dependencies, calls workflows
- Rendering and interaction logic lives here; business logic does not
- Never imported by anything else
