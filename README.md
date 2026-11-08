<div align="center">

<img src="assets/relay.png" alt="Relay" width="112" />

# Relay

**Local models. Real tools. One native desktop.**

A Rust-powered desktop workspace for AI coding agents on Linux. Relay brings a Codex-based agent, local inference through [Strata](https://github.com/Niko1221/Strata), and hands-on system control into a single, focused interface.

[![Rust](https://img.shields.io/badge/Rust-2024-orange?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Linux-444?logo=linux&logoColor=white)](#requirements)
[![License: MIT](https://img.shields.io/badge/License-MIT-444)](LICENSE)

</div>

---

## What is Relay?

Relay is a native frontend and management layer for a **local, tool-using coding agent**. Rather than stopping at a chat window, it gives the model a working directory, access to agent tools, and a way to carry out tasks on your machine—with permissions you control.

The pieces have distinct jobs:

- **Relay** provides the desktop UI, session controls, model lifecycle management, native helpers, and supervision.
- **Codex** provides the agent runtime: conversations, commands, file changes, approvals, skills, and other tools.
- **Strata** runs the local language model and exposes the inference endpoint used by the agent.

Relay does **not** bundle a model or replace the Strata inference engine or Codex agent runtime. It connects and manages them.

## Features

### A native Rust desktop

- **Rust + egui/eframe** interface, with no embedded browser or JavaScript frontend.
- **Single Relay executable**: the UI, supervisor, web adapter, clipboard handling, updater adapters, and other former helper scripts are built into the same binary.
- Dark, compact interface with Markdown rendering, a resizable chat drawer, system tray, and native file/folder picker.
- Attach local files or images, paste screenshots, and inspect generated file changes.

### A real coding agent

- Run a **Codex app-server** session against a local model served by Strata.
- Let the agent inspect and modify project files, run commands, and use available Codex tools.
- Choose a project directory, revisit conversations, and send additional context while a turn is running.
- Discover and attach agent skills; review tool activity and file diffs.
- Manage long conversations with context usage indicators and manual compaction (`/compact`).

### Local model control

- Connect to a configured Strata endpoint and discover the active model.
- Start and stop model services using configured lifecycle commands.
- Select locally configured model profiles without loading them just by opening the picker.
- Check for Strata component updates, use its verified update/rollback workflow, and configure available drafting modes.
- See model context and generation-speed information when the backend reports it.

### Access on your terms

Relay exposes four levels of agent access:

| Mode | Intended use |
| --- | --- |
| **Read only** | Explore and inspect without workspace writes. |
| **Ask for approval** | Work in the selected workspace, with approval prompts where required. |
| **Full access** | Allow the agent broader unsandboxed actions. |
| **Full access + sudo** | Additionally enable an explicitly authenticated privileged session. |

The optional sudo integration is tied to the GUI session. It does not install a permanent root service or a custom `sudoers` rule. **Full access modes can make consequential changes**; use them only when you trust the task and understand the commands being executed.

### Useful integrations

- **Persistent memory:** an editable local Markdown file for preferences and facts worth carrying across conversations.
- **Web tools:** an MCP search/read adapter for a separately running [OmniSearch](https://codeberg.org/gabrielf/omnisearch) service.
- **System telemetry:** optional CPU/GPU power, VRAM temperature and memory clock readings through existing RAPL/LACT interfaces.
- **Native Codex updater:** check for, install, or roll back a separately managed Codex agent build.

Some features depend on other software being installed or configured; Relay does not require every optional integration to be available.

## How it fits together

```text
                           Relay (Rust + egui)
                        Native Linux desktop UI
                                   │
                        Rust process supervisor
                                   │
                            Codex app-server
                                   │
                    Agent requests and tool calls
                          ┌────────┴────────┐
                          │                 │
                   Strata inference     Local agent tools
                   Local LLM endpoint   Files · shell · skills
                          │                 │
                       Your model       Your workspace

           Optional: OmniSearch MCP · sudo session · LACT telemetry
```

Relay's helper roles are implemented as internal subcommands of the same executable. The installed `relay-engine`, `relay-codex`, `relay-web`, and related compatibility names are symlinks—not separate Python utilities.

## Requirements

- **Linux** desktop with an X11 or Wayland environment.
- A current **Rust/Cargo toolchain** supporting Rust edition 2024, plus the native build dependencies needed for `eframe` on your distribution.
- A working **Strata** installation or accessible, appropriately configured Strata inference service.
- A compatible **Codex** executable. Relay can also manage a private Codex installation from its interface.

**Optional:** OmniSearch for web tools, LACT for additional hardware telemetry, and working sudo authentication for privileged actions.

> Relay is currently source-built and under active development. Behavior and configuration may change as the project evolves.

## Installation

Clone the repository and run the installer:

```bash
git clone https://github.com/mitschud/Relay.git
cd Relay
./install.sh
```

The installer compiles a release build if a prebuilt `bin/relay` is not present, installs the executable under `~/.local/opt/relay/`, and creates a desktop entry and a `~/.local/bin/relay` command.

Launch it from your application menu, or run:

```bash
relay
```

If your shell cannot find `relay`, make sure `~/.local/bin` is on your `PATH`, or launch `~/.local/opt/relay/relay` directly.

### Build without installing

```bash
cargo build --release --locked
./target/release/relay
```

For development and basic checks:

```bash
cargo check
cargo test
```

## First-time setup

1. **Prepare Strata.** Install/configure Strata and make sure your chosen model can be served through its local endpoint.
2. **Open Relay Settings.** Set the Strata endpoint and, if necessary, the model start/stop commands.
3. **Configure Codex.** Point `codex_binary` to an existing compatible executable, or use Relay's agent-management interface to install one.
4. **Select a project and model.** Choose the workspace directory and a configured model profile, then connect/load the agent.
5. **Choose an access mode.** Start with read-only or approval-based access until you need broader permissions.

The default settings location is:

```text
~/.config/relay/settings.json
```

A minimal example (adjust for your installation):

```json
{
  "base_url": "http://127.0.0.1:47831/v1",
  "alternate_urls": [],
  "codex_binary": "codex",
  "start_command": [],
  "stop_command": [],
  "preferred_model": "",
  "load_timeout_s": 600
}
```

`start_command` and `stop_command` are arrays of executable/argument strings, not shell command strings. Leave them empty if you manage Strata separately. Configured model profiles are read from `~/.config/relay/models.json`.

## Local files and data

| Path | Purpose |
| --- | --- |
| `~/.config/relay/settings.json` | Endpoint, agent, and lifecycle configuration. |
| `~/.config/relay/models.json` | Optional local model profiles. |
| `~/.local/share/relay/MEMORY.md` | Editable persistent memory. |
| `~/.local/share/relay/` | Relay-managed data, including its isolated Codex home/catalogue. |
| `~/.local/state/relay/` | Local lifecycle/lock state. |

Relay is designed for **local inference by default**, but it is not a promise of zero network traffic: explicit web-tool calls, update checks/downloads, or a non-local endpoint can contact external services.

## Development

The repository is a single Rust crate (`Cargo.toml`) with feature-focused modules under `src/`.

| Area | Key modules |
| --- | --- |
| Desktop interface | `ui`, `composer`, `drawer`, `attachments`, `picker` |
| Agent communication | `backend`, `model`, `skills`, `compaction` |
| Model lifecycle | `supervisor`, `engine`, `profiles`, `strata_helper` |
| Integrations | `codex_helper`, `web`, `clipboard_helper`, `local_model` |
| System and safety | `sudo`, `fingerprint`, `instance`, `tray` |
| Runtime feedback | `metrics`, `telemetry`, `activity` |

The design deliberately keeps the rendering thread separate from slower I/O work, while background operations report their results back to the UI.

Contributions, bug reports, and feedback are welcome through [GitHub Issues](https://github.com/mitschud/Relay/issues).

## License

Distributed under the [MIT License](LICENSE).
