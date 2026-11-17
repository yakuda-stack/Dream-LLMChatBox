# AUR: dream-llmchatbox

Paket: https://aur.archlinux.org/packages/dream-llmchatbox (nach dem ersten Push)
Installation für Nutzer: `yay -S dream-llmchatbox` (oder paru).

Das PKGBUILD baut die App aus dem GitHub-Tag (Rust, `cargo build --frozen --release`):

| Was | Wohin |
|---|---|
| Programm | `/usr/bin/dream-llmchatbox` |
| Startmenü | `/usr/share/applications/dream-llmchatbox.desktop` |
| Icon | `/usr/share/icons/hicolor/256x256/apps/…png` + `scalable/apps/…svg` |
| Lizenz + Doku | `/usr/share/licenses/…`, `/usr/share/doc/…` |

Läuft die App aus `/usr`, schreibt sie **keinen** eigenen Startmenü-Eintrag nach
`~/.local/share/applications` (ein alter aus `cargo run` wird entfernt) – dann gilt der
Eintrag aus dem Paket.

---

## Einmalig: das Paket im AUR anlegen

1. **AUR-Account + SSH-Key** – hast du schon (gleicher Key wie bei den anderen Paketen).
2. **GitHub:** Projekt pushen und den Tag setzen – das PKGBUILD lädt
   `…/archive/refs/tags/v0.1.0.tar.gz`, ohne Tag gibt es 404.
   (Release-Launcher → Dream-LLMChatBox → 3 · GitHub → Commit + Push, Tag setzen + Push)
3. **Leeres AUR-Repo klonen** – der Name wird dabei reserviert:
   ```bash
   git clone ssh://aur@aur.archlinux.org/dream-llmchatbox.git ~/aur/dream-llmchatbox
   ```
   („warning: You appear to have cloned an empty repository“ ist richtig so.)
4. **.gitignore anlegen** (nur PKGBUILD + .SRCINFO kommen ins AUR):
   ```bash
   cp packaging/aur/gitignore-fuer-aur-repo ~/aur/dream-llmchatbox/.gitignore
   ```
5. Ab hier wie bei jedem Update (unten) – im Release-Launcher Gruppe **5 · AUR** von oben nach unten.
   Beim allerersten Commit zusätzlich die .gitignore mitnehmen:
   ```bash
   cd ~/aur/dream-llmchatbox && git add .gitignore
   ```

## Update-Ablauf (neue Version) – Release-Launcher, Projekt „Dream-LLMChatBox“

1. **1 · Version:** Version eintragen → „Version setzen“
   (setzt `Cargo.toml`, `Cargo.lock`, `packaging/aur/PKGBUILD`, pkgrel=1)
2. `CHANGELOG.md` oben einen Block `## [vX.Y.Z] – Datum` schreiben
3. **2 · Testen:** „Alle Tests nacheinander“ (cargo test, cargo clippy, Desktop-Datei, shellcheck, namcap)
4. **3 · GitHub:** Verbotene Dateien? → Commit + Push → Tag setzen + Push
5. **4 · Release:** AppImage bauen → Quell-Archiv packen → GitHub-Release-Seite öffnen
   (hochladen: AppImage + .zsync + .tar.gz, KEIN Pre-release)
6. **5 · AUR:** PKGBUILD kopieren → Aufräumen → Version prüfen → updpkgsums
   → Bauen + installieren → Installiertes Paket testen → .SRCINFO + Commit → AUR Push

Von Hand (ohne Launcher):
```bash
cp packaging/aur/PKGBUILD ~/aur/dream-llmchatbox/
cd ~/aur/dream-llmchatbox
updpkgsums                              # holt die sha256 des Tags
makepkg -si                             # bauen + installieren, dann testen
makepkg --printsrcinfo > .SRCINFO
git add PKGBUILD .SRCINFO && git commit -m "Update to v0.1.0" && git push origin master
```

## Regeln / Stolperfallen
- **Tag zuerst**, sonst 404 bei `updpkgsums`.
- **Cargo.lock muss im GitHub-Repo sein** – das PKGBUILD baut mit `--frozen`.
- **.SRCINFO** bei JEDER Änderung neu erzeugen – sonst lehnt das AUR den Push ab.
- **pkgrel:** neue App-Version → 1; nur Packaging geändert → +1.
- **pkgver** ohne Bindestrich: `0.5.0_alpha` (der Tag `v0.5.0-alpha` wird über `_tag` gebaut).
- Ins AUR-Repo **nur** PKGBUILD + .SRCINFO (+ .gitignore) – nie Quellcode, nie `src/`, `pkg/`, `*.pkg.tar.zst`.
- Branch: Projekt = `main`, AUR = `master`.
- Der erste Build dauert ein paar Minuten (whisper.cpp + Slint werden mitgebaut).
