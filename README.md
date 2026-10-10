# Meca Launcher

> **NOT AN OFFICIAL MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT.**

Meca Launcher is a small Windows launcher for one private, friends-only
*Minecraft: Java Edition* server ("Meca", Forge 1.20.1). It keeps every
player's modpack identical to the server's and then starts the game.

It is free, shows no ads and is not sold or monetised in any way.

This repository holds the modpack list (`manifest.json`) that the launcher
reads. It contains **no game files and no mod files**.

## What the launcher does

1. **Signs the player in** with their own Microsoft account.
2. **Checks that the account owns the game.** Accounts without
   *Minecraft: Java Edition* cannot play.
3. **Installs what is missing** (Java 17, Minecraft 1.20.1, Forge 47.4.10 and
   the mods), each file from its official source.
4. **Keeps the mods in sync** with `manifest.json`: downloads what changed and
   removes what is no longer part of the pack.
5. **Starts the game** and joins the server.

## Accounts and sign-in

- **Microsoft accounts only.** Sign-in uses the Microsoft identity platform:
  OAuth 2.0 authorization code flow with PKCE, in the player's own web
  browser. The launcher never sees, asks for or stores a password.
- **Public client.** There is no client secret.
- **Scope requested:** `XboxLive.signin offline_access`.
- **Token chain:** Microsoft account → Xbox Live → XSTS → Minecraft Services
  (`/authentication/login_with_xbox`, `/entitlements/mcstore`,
  `/minecraft/profile`).
- **No offline mode.** There is no username-only login, no "cracked" mode and
  no way to skip the ownership check.

Azure application (client) ID: `020af44f-9252-4229-9178-04253249cdb6`

## Where the files come from

| What | Source | How it is verified |
| --- | --- | --- |
| Minecraft client, libraries and assets | Mojang's download servers, the same ones the official launcher uses | SHA-1 published by Mojang |
| Java 17 runtime | Mojang's download servers | SHA-1 published by Mojang |
| Forge | The official installer from `maven.minecraftforge.net` | SHA-1 pinned in `manifest.json` |
| Mods | Each mod's own file on Modrinth (`cdn.modrinth.com`) | SHA-512 pinned in `manifest.json` |

Every player downloads these files directly from those sources. Nothing is
re-hosted, modified or redistributed by this project.

## Privacy

No telemetry, no analytics and no accounts of our own. Sign-in tokens stay on
the player's PC. See [PRIVACY.md](PRIVACY.md).

## Contact

Open an issue in this repository.

---

## En español

Meca Launcher es un launcher gratuito para un servidor privado de Minecraft
entre amigos. Solo admite cuentas de Microsoft que ya tienen el juego, descarga
todos los archivos de sus fuentes oficiales y no guarda ni envía datos de los
jugadores a nadie.

**No es un producto oficial de Minecraft. No está aprobado por Mojang o
Microsoft ni asociado con ellos.**
