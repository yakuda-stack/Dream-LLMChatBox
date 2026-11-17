# Changelog

All notable changes to Dream-LLMChatBox. Newest first.

## [v0.1.0] – 2026-11-17

First public release (GitHub, AppImage, AUR). Everything below was built before it, sorted by date.

### Added
- Release files: AppImage with delta updates (`scripts/build_appimage.sh`), AUR package (`packaging/aur/PKGBUILD`), `scripts/bump_version.py`
- `dream-llmchatbox --version` and `--help`
- Desktop entry points to the AppImage file itself; with the AUR package the system entry is used
- `install.sh`: one-line install with `curl … | bash` (AppImage to `~/.local`, `--uninstall`)
- Appearance setting: System (default), Light, Dark
- New README with screenshots, feature overview and contributors

## 2026-11-16 (2)

### Added
- Sidebar: pin chats, move them up/down or drag them at a handle, assign to a group by right-click
- Multi-select (Ctrl + click) of chats and groups and "Merge into new group" that combines their memories
- Auto-Switch AI Model (BETA) in Settings: a small router model picks the best installed model per message, a fitting loaded model is kept
- Context bar (green/orange/red) below the input, can be switched off
- Work folder per group and chat with a copy of a read-only project, asked once per chat
- Standard start argument "ask when unknown" against made-up answers
- ARCHITEKTUR.md
- Thinking switch per chat (Auto / On / Off; Auto turns thinking off for simple chat) and collapsible "💭 Thinking" box
- Work folder diff view with "Apply to project"
- "+" → "🧪 Test environment": create the chat's work folder with a project copy on purpose, after a confirmation that shows the path
- Chat search, export as Markdown, automatic chat titles
- Model reads ARCHITEKTUR.md first in every chat with a shared folder and keeps it up to date
- Right-click menu Copy / Select all in chat messages
- Graphics backend switch (Vulkan / ROCm / CPU) in the Ollama settings
- Whole app in German and English (Slint bundled translations + `i18n::tr` for Rust texts)
- Step-by-step tour with language choice on the first start (replaces the ⓘ hints), restart in Settings
- "Reset everything" in Settings with a warning dialog
- Help and one retry when Ollama's model runner crashes
- "🧪 Test environment" switch in a new chat
- Optional Podman/Docker container for commands in test environments (off by default)
- "Install" button when Ollama is missing
- Automations moved into the sidebar below the chat search
- Window icon on Wayland fixed (app id is now set after the window exists)
- Settings window scrolls, long hints no longer overlap
- Automations: scheduled tasks (daily, weekdays, weekly, every few hours), result as a new chat + desktop notification
- Live text while speaking (realtime speech input, can be switched off in the microphone panel)
- Compress long chats automatically (default at 80 %) or with "🗜 Compress"; summary box in the chat
- Create documents with `create_document` (PDF, DOCX, ODT, PPTX, HTML, EPUB via pandoc)
- Searchable documents (RAG) per group with `search_documents`, Ollama embeddings and automatic re-indexing
- Models → Servers tab: OpenAI-compatible servers (LM Studio, llama.cpp, vLLM, LocalAI) with streaming and tools
- Global memory with `list_projects` and `request_project_access` (asks read / read + write / no)
- Architecture index from ARCHITEKTUR.md in the prompt and in the group memory
- Models → Online tab: Claude Code, ChatGPT (Codex), Gemini and Grok through their CLIs (install, log in, activate)
- Lists with real hanging indent
- Checkpoints with "↶ Undo file changes" for the newest answer
- Preview tab (images, PDF, text, HTML as text) in the side panel
- Start argument "Answer like an online AI" with a style picker (Claude, Gemini, ChatGPT, Grok, DeepSeek); styles are editable Markdown files in `answer_styles/`
- App icon in the top bar, window icon and desktop entry (Wayland app id `dream-llmchatbox`)
- Start arguments: short titles with an "i" for the full text
- Group settings: remove single global start arguments for one group; test environment for every chat of a group
- Group settings: switch on globally-off start arguments only for one group
- Standard start argument "no comments in code" (off by default)
- Terminal commands may run up to 15 minutes (was 3), with a live timer in the activity
- Tool `pause_and_wait` with Continue button and optional automatic continue (Settings → Pauses)
- Loop guard against the model repeating itself
- Tool `ask_user`: the model asks with up to 4 clickable answers plus your own answer
- Tools `search_files` (grep over shared folders) and `edit_file` (replace a unique part of a file, approval shows − old / + new)
- Context saver: no second `read_file` of an unchanged file in one answer, old tool results shortened when the context is over ~55 % full, `<think>` blocks left out of the history

### Changed
- Hardware info shows only the active GPU with its exact name from `amdgpu.ids` (e.g. RX 9070 XT instead of "9070/9070 XT/9070 GRE")
- Tool steps in a row raised from 5 to 10
- Activity list in the side panel scrolls; the extra 🌐 button at the top is gone (Browser is a tab in the side panel)

### Removed
- Profile passwords. A profile that still has one asks for it once and is then stored without a password

## 2026-11-16

### Added
- MCP servers (stdio): tools of standard MCP servers are offered to the model as `mcp_<server>_<tool>`, every call asks first; managed on the Tools page (add, edit, delete, on/off, "Test", "Import from Claude Desktop", "Open mcp.json"), saved in `~/.config/dream-llmchatbox/mcp.json` (Claude Desktop format)
- `write_file` asks for approval before creating or overwriting a file
- Coding mode: purpose "Other / Coding" per chat and per group, "💻 Coding" chip, setup bar with "I have an ARCHITEKTUR.md" / "No ARCHITEKTUR.md yet" / "Brand new project"; the architecture file and ROADMAP.md are given to the model first, otherwise it writes ARCHITEKTUR.md from a fixed template
- Profiles: own chats, groups, memory, start arguments, name, "about you" and picture per profile; "Switch profile…" in the profile menu; existing data is moved into the profile "default"
- Optional profile password with real encryption (Argon2id + XChaCha20-Poly1305); unlocked data only in RAM, unlock screen at start
- Groups page with a box per group; per group the global start arguments and the memory can be switched off
- Activity is saved per chat: the side panel shows only the steps of the open chat, also after a restart
- CHANGELOG.md
- Groups (e.g. one per project) with chats listed under the group, a standard folder, own rules, export/import and per-group memory as Markdown files
- Memory tools `memory_list`, `memory_search`, `memory_read`, `memory_write` for chats in a group
- Web tools `web_search` (DuckDuckGo) and `open_web_page` (clean text), both with approval; local and home-network addresses are blocked
- Browser panel (🌐) that shows every search and page the model opens; you can browse yourself too
- Activity indicator in the top bar and an activity list in the side panel
- Terminal tool `run_terminal_command` with an approval bar ("Yes, once" / "Allow in this chat" / "No"), a list of safe commands and a blocklist
- Tools page: switch every tool on/off, edit built-in tools (description, parameters, command), add own bash tools, import/export them; every tool is a file in `~/.config/dream-llmchatbox/tools/`
- Start arguments page with standard entries and the `remember` tool
- Folder access per chat with sandboxed tools `list_folder`, `read_file`, `write_file`
- Tool calling with an agent loop (up to 5 steps), tool calls shown as boxes in the chat
- Bouncing dots while the model thinks or writes
- Shared folders are shown on your messages

### Changed
- Chat database moved from `~/.config/dream-llmchatbox/` to `~/.local/share/dream-llmchatbox/` (automatic)
- Read aloud, copy, continue and token count are inside the answer bubble
- The edit pencil is on your last message instead of the answer
- The text input wraps long text and grows up to about 8 lines; Shift+Enter makes a new line

### Fixed
- Long answers with many wrapped lines were cut off at the bottom
- The send button disappeared when a message was very long
- "Not enough VRAM": other models are unloaded first, then the context is halved until the model fits (remembered per model)

## 2026-11-15

### Added
- Chat window with streaming answers, model selection, stop button
- Chat history in SQLite with rename, delete (frees the disk space), pagination and "Delete all chats"
- Collapsible sidebar, top bar, remembering the last model
- Models page: installed and available models (Hugging Face and ollama.com search), hardware estimate, test button, install in a terminal
- Attachments: files, folders, images, audio/video (transcribed), Ctrl+V for images and files
- Microphone input with whisper.cpp and a settings panel
- Code blocks with Copy, Markdown cleanup, read aloud with Piper voices
- Context length setting with "Continue" near the limit
- Profile menu: profile, settings (answer language), Ollama settings, start/stop Ollama
- Images as text (colors + OCR) for models without image support
