<div align="center">

# Model Picker

**One Load button. Multiple local models. No accidental launches.**

Manage model profiles in Relay's native desktop interface, switch between inference backends, and keep model startup under your control.

[Back to Relay](README.md) · [Configuration](#configuration) · [Profile reference](#profile-reference) · [Troubleshooting](#troubleshooting)

</div>

---

## Overview

Relay's **Load** control has two actions:

- **Dropdown arrow:** open the model picker and select a profile.
- **Load:** connect using the currently selected profile, starting its configured backend service if needed.

**Selecting a profile does not start, stop, or download a model.** It updates Relay's saved settings and stores the selected profile's `id` as `model_profile_id`. The actual model lifecycle begins only when you press **Load** (or activate Load with Enter).

Profiles are optional. Without a `models.json` file, Relay still works as a single-model client using its existing `settings.json` configuration.

### How the pieces fit together

```text
~/.config/relay/models.json
           │
           ▼
   Model picker (UI)
      ├── Display installed / preparing / demo profiles
      └── Select a profile ──► merge settings into settings.json
                                      │
                                Click Load
                                      │
                                      ▼
                              Engine supervisor
                               ├── Check readiness
                               ├── Detect conflicting services
                               ├── Connect or start backend
                               └── Launch Codex app-server
                                      │
                                      ▼
                              Local inference API
```

Relay manages the connection; **model weights, downloaded runtimes, and user services are provided separately**.

## Configuration

| File | Purpose |
| --- | --- |
| `~/.config/relay/models.json` | Optional list of selectable model profiles. |
| `~/.config/relay/settings.json` | Active settings used when **Load** is pressed. |
| Paths referenced by each profile | Your own model files, runtime configuration, or preparation markers. |

Create the configuration directory if necessary:

```bash
mkdir -p ~/.config/relay
```

Create `~/.config/relay/models.json` and add a `profiles` array. Each profile needs a unique `id`, a display `name`, and a `settings` object.

### Example: Strata and llama.cpp

The service names, ports, model identifiers, and paths below are **examples**, not services or models installed by Relay. Change them to match your machine.

```json
{
  "profiles": [
    {
      "id": "qwen-flash",
      "name": "Qwen Flash Next",
      "detail": "Strata · UD-IQ4_XS · local inference",
      "required": ["/home/USER/models/qwen-flash/ready.json"],
      "settings": {
        "backend": "strata",
        "base_url": "http://127.0.0.1:47832/v1",
        "alternate_urls": ["http://127.0.0.1:47831/v1"],
        "preferred_model": "",
        "strata_model_config": "/home/USER/models/qwen-flash/run.json",
        "model_required": ["/home/USER/models/qwen-flash/ready.json"],
        "start_command": ["systemctl", "--user", "start", "--no-block", "strata-headless.service"],
        "stop_command": ["systemctl", "--user", "stop", "strata-headless.service"],
        "exclusive_services": ["strata-headless.service", "relay-llama.service"],
        "exclusive_endpoints": ["http://127.0.0.1:47834/v1"]
      },
      "card": {
        "title": "Qwen Flash",
        "subtitle": "MoE · Strata",
        "quant": "UD-IQ4_XS"
      }
    },
    {
      "id": "llama-local",
      "name": "Local llama.cpp",
      "detail": "llama.cpp · local GGUF",
      "required": ["/home/USER/models/local/ready.json"],
      "settings": {
        "backend": "llama.cpp",
        "base_url": "http://127.0.0.1:47834/v1",
        "alternate_urls": [],
        "preferred_model": "",
        "local_model_config": "/home/USER/.config/relay/local-model.json",
        "model_required": ["/home/USER/models/local/ready.json"],
        "start_command": ["systemctl", "--user", "start", "--no-block", "relay-llama.service"],
        "stop_command": ["systemctl", "--user", "stop", "relay-llama.service"],
        "exclusive_services": ["strata-headless.service", "relay-llama.service"],
        "exclusive_endpoints": ["http://127.0.0.1:47832/v1"]
      },
      "card": {
        "title": "Local GGUF",
        "subtitle": "llama.cpp",
        "quant": "Q4_K"
      }
    }
  ]
}
```

Replace `USER` with your username and use existing files for readiness markers. **Do not copy a nonexistent `ready.json` path verbatim**: Relay will treat the profile as not ready.

For a backend you launch manually, omit model-management commands or set them to empty arrays:

```json
{
  "start_command": [],
  "stop_command": []
}
```

In that configuration, the inference endpoint must already be running before pressing **Load**.

### Selecting and connecting

1. Open Relay and click the arrow on the white **Load** control.
2. Select an available model card. This applies that profile's `settings` to `settings.json` without launching an engine.
3. Press **Load** to connect. Relay checks the endpoint and runs the configured start command only when needed.
4. When you are finished, use the normal unload/disconnect workflow. The supervisor's cleanup behavior depends on whether it started the backend or is configured to manage an existing one.

If you add or edit profiles while Relay is open, reopen the picker with the arrow; it reloads `models.json` when opened.

## Profile reference

| Field | Required | Description |
| --- | --- | --- |
| `id` | Yes | Stable identifier saved in `settings.json` as `model_profile_id`. Keep unique. |
| `name` | Yes | Display name in the picker. |
| `detail` | No | Additional description displayed as hover text. |
| `settings` | Yes | JSON object merged **one level deep** into active `settings.json` on selection. |
| `required` | No | Paths checked by the picker to indicate a missing or unfinished installation. |
| `placeholder` | No | `true` makes the card a demo entry that cannot load. |
| `card.title` | No | Primary text on the model card; defaults to `name`. |
| `card.subtitle` | No | Secondary text (for example, model family or size). |
| `card.quant` | No | Quantization label displayed on the card. |
| `card.quant_from_detail` | No | Derive a quant label from the segment after the first `·` in `detail` instead of using `card.quant`. |

### Common `settings` keys

| Key | Meaning |
| --- | --- |
| `backend` | `"strata"` or `"llama.cpp"`; controls backend-specific UI behavior. |
| `base_url` | OpenAI-compatible model API endpoint, typically ending in `/v1`. |
| `alternate_urls` | Additional model API endpoints Relay may try. |
| `preferred_model` | Exact exposed model ID to select, or `""` for automatic discovery. |
| `start_command` | Executable and arguments to launch the backend when unavailable. |
| `stop_command` | Executable and arguments used by lifecycle cleanup where applicable. |
| `strata_model_config` | Path to the Strata model configuration used by Relay's Strata integration. |
| `local_model_config` | Configuration consumed by Relay's compiled-in `relay-local-model` helper. |
| `model_required` | **Supervisor-side** file checks before connection. |
| `exclusive_services` | Other user-level systemd units that should not be active when launching this backend. |
| `exclusive_endpoints` | Other model APIs to check for an already running model before launch. |
| `load_timeout_s` | Maximum time to wait for the model endpoint during startup. |

**Important: `required` and `model_required` serve different purposes.** The profile's top-level `required` list is checked by the **picker**. The `settings.model_required` list is checked by the **engine supervisor**. Specify both when you want the same readiness protection in the interface and at startup.

Profile selection replaces the individual `settings` keys you provide and preserves other existing keys in `settings.json`. It does **not** recursively merge nested objects. To avoid accidentally inheriting a previous profile's values, set backend-specific fields explicitly where applicable.

### Card presentation

The picker displays compact cards with a title, subtitle, quant label, and backend. The selected profile gets an accent border/check mark. Cards can be shown in up to two columns, depending on the number of entries.

A profile without a `card` object still works; `name` is used as its title.

To add a nonfunctional preview entry:

```json
{
  "id": "coming-soon",
  "name": "Future model",
  "detail": "Not installed",
  "placeholder": true,
  "settings": {},
  "card": {
    "title": "Coming soon",
    "subtitle": "Preview only"
  }
}
```

## Backend notes

### Strata

Relay discovers the actual model exposed by the configured endpoint, then passes that model and endpoint to its Codex agent runtime. A picker card is a **local configuration preset**, not a downloadable model catalogue. The `preferred_model` field can constrain discovery to a particular reported model ID.

Your `strata-headless.service` and its model configuration must be created separately. Relay does not install that sample service or download its model weights.

### llama.cpp

Setting `backend` to `"llama.cpp"` selects Relay's local-runtime interface instead of Strata-only configuration controls.

Relay includes a compiled-in `relay-local-model` helper. Given a `local_model_config`, it reads a JSON configuration, verifies any listed `required` files, locates **exactly one** `llama-server` binary under the configured runtime root, constructs `LD_LIBRARY_PATH` for colocated shared libraries, removes certain software-rendering environment overrides, and executes the server.

A minimal helper configuration has this shape:

```json
{
  "runtime_root": "/home/USER/.local/opt/llama-runtime",
  "required": ["/home/USER/models/local/ready.json"],
  "args": [
    "--model", "/home/USER/models/local/model.gguf",
    "--host", "127.0.0.1",
    "--port", "47834"
  ]
}
```

The service in `start_command` must invoke the local-model helper appropriately. The example **does not create a systemd unit** or install llama.cpp for you. Make sure the server supports the API used by Relay's Codex runtime.

## Safety and lifecycle

Relay tries to avoid conflicting model instances when starting a backend:

- `exclusive_services` checks other configured user-level systemd services.
- `exclusive_endpoints` checks configured APIs for loaded models.
- The supervisor coordinates engine startup and maintains session/lifecycle state.

**Relay does not kill an unrelated client's active model to make room.** A detected conflict results in an error so you can stop the other service intentionally. Conflict checks are relevant to backend startup; they are not an unconditional guarantee that every external process is detected.

Commands are **argument arrays**, not shell command strings. For example:

```json
{
  "start_command": ["systemctl", "--user", "start", "my-model.service"]
}
```

Do not write `"start_command": "systemctl --user start my-model.service"`.

## Troubleshooting

| Symptom | What to check |
| --- | --- |
| No dropdown appears | Make sure `~/.config/relay/models.json` exists and contains a valid `profiles` array with `id`, `name`, and `settings` for each entry. |
| Profile says it is preparing | At least one file in the profile's top-level `required` array does not exist. |
| Profile is marked demo-only | Set `placeholder` to `false` or remove it after installing the model. |
| Profile selection works, but Load fails | Confirm the active `base_url`, server API, `start_command`, and any `model_required` markers. |
| Another model is active | Check the services and APIs listed in `exclusive_services` and `exclusive_endpoints`; unload other sessions first. |
| The wrong model is discovered | Check `preferred_model` against the IDs reported by the endpoint's `/models` API. |
| `llama-server` cannot be found | Check `local_model_config`, `runtime_root`, and that exactly one `llama-server` exists beneath it. |
| Changes do not appear in the picker | Close and reopen the dropdown to reload `models.json`. |
| JSON fails to load | Validate it with `python -m json.tool ~/.config/relay/models.json`. Standard JSON does not allow comments or trailing commas. |

## Related documentation

See the main [README](README.md) for installation, the Codex/Strata architecture, permissions, and general first-time setup.

---

<div align="center">

**Configure locally. Select intentionally. Load when ready.**

</div>
