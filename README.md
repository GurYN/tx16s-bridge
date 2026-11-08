# TX16S Bridge

Utilise une RadioMaster TX16S branchée sur le Mac dans un jeu qui tourne sur le PC
et qui est streamé vers le Mac (Steam Remote Play, Moonlight…). Le stream ne
transporte pas la radio : ce pont l'envoie séparément sur le réseau local, et le PC
la présente au jeu comme un joystick vJoy.

```
TX16S ──USB──▶ Mac : TX16S Bridge.app ──UDP 47800──▶ PC : service tx16s-receiver ──▶ vJoy ──▶ jeu
                         ▲                                        │
                         └──────────── état (vJoy, pertes) ───────┘
```

| Dossier | Rôle |
|---|---|
| `crates/protocol` | Format des paquets, commun aux deux programmes |
| `crates/sender` | App Mac (gpui) : lit la radio, l'envoie, affiche voies, boutons et état du PC |
| `crates/receiver` | Service Windows : reçoit, pilote vJoy, renvoie son état |
| `python/` | Premiers scripts, compatibles avec le même protocole |

## Mac

```sh
./scripts/bundle-macos.sh          # crée target/release/TX16S Bridge.app
cp -R "target/release/TX16S Bridge.app" /Applications/
```

Au premier lancement, macOS demande l'autorisation **Surveillance de l'entrée** pour
lire la radio. Sur la radio, choisis **USB Joystick** au branchement. Saisis l'adresse
du PC (`10.0.0.200`, ou `10.0.0.200:port`), puis **Démarrer**. L'adresse et l'état
démarré/arrêté sont mémorisés.

La case « signal de test » remplace la radio par des sinusoïdes, pour vérifier le PC
sans elle. `--test` au lancement l'active d'emblée.

## PC Windows

1. Installer [vJoy](https://github.com/BrunnerInnovation/vJoy/releases) puis, dans
   **Configure vJoy**, le périphérique 1 avec 8 axes et 32 boutons.
2. Installer Rust (`rustup`, avec les Build Tools Visual Studio) puis, dans ce dossier :
   ```
   cargo build --release -p tx16s-receiver
   ```
3. Dans un terminal **administrateur** :
   ```
   target\release\tx16s-receiver.exe install
   ```
   Le service est copié dans `C:\Program Files\tx16s-bridge\`, démarre avec Windows,
   et une règle de pare-feu (profil **Privé**) l'autorise. Le réseau Windows doit donc
   être classé Privé.

Autres commandes : `status`, `stop`, `start`, `uninstall`, et `run` pour le lancer
dans le terminal (`run --dry-run` fonctionne sans vJoy, sur n'importe quel OS).

Configuration : `C:\ProgramData\tx16s-bridge\config.toml` (port, numéro vJoy,
adresse du Mac autorisée, délai du failsafe). Journaux :
`C:\ProgramData\tx16s-bridge\logs\`.

Si le PC ne reçoit plus rien du Mac pendant 500 ms, le service recentre les axes et
relâche les boutons.

## Développement

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo check -p tx16s-receiver --target x86_64-pc-windows-gnu   # code Windows, depuis le Mac
```

gpui compile ses shaders Metal pendant le build : il faut Xcode complet et son
composant Metal (`xcodebuild -downloadComponent MetalToolchain`). Si `xcode-select`
pointe vers les Command Line Tools, préfixer cargo de
`DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer` ;
`scripts/bundle-macos.sh` le fait tout seul.

L'icône est dessinée par `scripts/make-icon.swift`, qui régénère
`assets/AppIcon.icns` et `assets/AppIcon.png`.
