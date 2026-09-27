<p align="center">
  <img src="resources/icons/128x128@2x.png" width="128" height="128" alt="Fleqi app icon" />
</p>

<h1 align="center">Fleqi</h1>

<p align="center">
  <strong>Open-source desktop command assistant for macOS</strong>
</p>

<p align="center">
  Work in your current Finder folder and selection with natural language, <code>!</code> commands,<br />
  and a persistent interactive terminal.
</p>

<p align="center">
  <a href="https://github.com/Fleqi-App/fleqi/releases/tag/v0.0.2"><strong>Download</strong></a>
  &nbsp;·&nbsp;
  <a href="README_EN.md">English</a>
  &nbsp;/&nbsp;
  <a href="README_CH.md">中文</a>
  &nbsp;·&nbsp;
  <a href="docs/README.md">Docs</a>
</p>

<p align="center">
  <a href="https://github.com/Fleqi-App/fleqi/releases/tag/v0.0.2"><img alt="Release 0.0.2 Beta" src="https://img.shields.io/badge/release-0.0.2_Beta-2c5282?style=flat-square" /></a>
  <a href="LICENSE"><img alt="AGPL-3.0-only license" src="https://img.shields.io/badge/license-AGPL--3.0--only-2f855a?style=flat-square" /></a>
  <a href="https://github.com/Fleqi-App/fleqi/stargazers"><img alt="GitHub stars" src="https://img.shields.io/github/stars/Fleqi-App/fleqi?style=flat-square&color=805ad5" /></a>
</p>

<p align="center">
  <img alt="macOS 14 or later" src="https://img.shields.io/badge/macOS-14%2B-1f425f?style=flat-square&logo=apple&logoColor=white" />
  <img alt="Apple Silicon" src="https://img.shields.io/badge/Apple_Silicon-supported-555555?style=flat-square&logo=apple&logoColor=white" />
  <img alt="Windows planned" src="https://img.shields.io/badge/Windows-planned-0078d4?style=flat-square&logo=windows&logoColor=white" />
  <img alt="Linux planned" src="https://img.shields.io/badge/Linux-planned-fcc624?style=flat-square&logo=linux&logoColor=black" />
</p>

<p align="center">
  <img alt="Tauri 2" src="https://img.shields.io/badge/Tauri-2-24c8db?style=flat-square&logo=tauri&logoColor=white" />
  <img alt="Rust 2024" src="https://img.shields.io/badge/Rust-2024-ce422b?style=flat-square&logo=rust&logoColor=white" />
  <img alt="React 19" src="https://img.shields.io/badge/React-19-149eca?style=flat-square&logo=react&logoColor=white" />
</p>

The input bar is the everyday entry point; the workspace holds sessions, tasks, capabilities, and settings. You configure your own model API or local service, without a Fleqi account or subscription. [Release notes](docs/release-notes/0.0.2.md) and [progress and acceptance](docs/status.md) provide current details.

## What Fleqi does

- **Natural-language tasks:** Create a reviewable plan and confirm or run it under the selected AI policy. You provide the model and credentials.
- **Manual commands and terminal:** Submit a command with `!`, or use a persistent terminal for a shell, vim, REPL, and other interactive programs. Both user-input paths go directly to the terminal.
- **Finder context and sessions:** The visible session follows Finder's directory, waiting for a safe prompt when the terminal is busy or an input line is unfinished. Each session keeps its own directory, history, and terminal.
- **Local capabilities and management:** Work with files, ZIP archives, images, audio and video, PDFs, text, and documents; manage capabilities, rules, favorites, tools, and settings. The [capability catalog](docs/capabilities.md) and [progress record](docs/status.md) define scope and acceptance conditions.

The first public beta is for macOS. Windows, Linux, and live voice are planned for later versions. See [docs/status.md](docs/status.md) for the authoritative acceptance record for the complete first version.

## Interface and references

[Workspace reference screenshot](Web%20APP/docs/previews/core-workspace.png) · [General settings reference screenshot](Web%20APP/docs/previews/core-settings-general.png) · [UI and interaction design](docs/ui-design.md)

These screenshots come from the `Web APP/` visual reference project; they are not captures of the current app. The [UI design record](docs/ui-design.md#13-视频证据登记) documents the local input-bar and task-overlay video references; the video is not distributed with the repository. The icon above comes from the committed [app icon resources](resources/icons/MANIFEST.json); see [Icon/README.md](Icon/README.md) for its design source.

## Develop locally

The repository root is the Tauri 2, Rust, React/TypeScript, and Vite workspace. See the [contribution guide](CONTRIBUTING.md) and [development documentation](docs/README.md) for the macOS environment and full command list. Common commands:

```bash
pnpm install --frozen-lockfile
pnpm run doctor
pnpm check
pnpm tauri dev
```

Use `pnpm test:ui` for UI tests and `pnpm test:desktop` for native app tests. The [release guide](docs/releasing.md) covers packaging and signed updates. The `Web APP/` preview and tests apply only to the visual reference project.

## Documentation and repository

| Entry | Contents |
|---|---|
| [Requirements and acceptance](docs/requirements.md), [capability catalog](docs/capabilities.md) | Product behavior, defaults, capability inputs and outputs, acceptance criteria |
| [UI design](docs/ui-design.md), [architecture and interfaces](docs/architecture.md) | UI states, interactions, modules, and data boundaries |
| [Development plan](docs/development-plan.md), [progress record](docs/status.md) | Implementation and verification requirements, results, and remaining work |
| [Desktop host](apps/desktop/), [Web UI](packages/ui/), [Rust crates](crates/), [type contracts](packages/contracts/) | Current app source; Rust DTOs are the source of generated types |
| [Reference UI](Web%20APP/README.md), [icon design source](Icon/README.md) | Visual references, not app acceptance evidence |

## Commit targets and content requirements

Target the `main` branch. When committing or opening a PR against `main`, include the files required by the change:

| Change | Required content |
|---|---|
| README copy | Update `README_CH.md` and `README_EN.md` together, and keep the language links in `README.md` working. Do not restore a project status section; link to the [progress record](docs/status.md) instead. |
| Product behavior or UI | Update the affected [requirements, capability, UI, architecture, and acceptance documents](docs/README.md) together; keep IDs, defaults, and references consistent. |
| App implementation or Rust DTO | Commit the source and describe appropriate verification results in the PR. For DTO changes, run `pnpm contracts:regen` and commit the [generated TypeScript bindings](packages/contracts/src/bindings/) too. |
| Images, references, or release information | Link only to committed, accessible assets. Update the [icon provenance record](resources/icons/MANIFEST.json) with derived icons, and align release claims with the [release guide](docs/releasing.md) and [progress record](docs/status.md). |

The [contribution guide](CONTRIBUTING.md) lists checks to run before submitting changes.

## License

Fleqi is licensed under [AGPL-3.0-only](LICENSE). See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for third-party sources and licenses.
