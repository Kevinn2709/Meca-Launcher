# Meca Launcher: privacy

> NOT AN OFFICIAL MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT.

Meca Launcher has no servers of its own and collects nothing. This page says
exactly what it handles and where it stays.

## What the launcher handles

- **Your Microsoft sign-in.** It happens on Microsoft's website, in your own
  browser. The launcher never sees your email address or your password.
- **A renewal token**, so you do not have to sign in every time. It is stored
  in the Windows Credential Manager on your PC, under `com.meca.launcher`.
- **A Minecraft session token**, needed to join the server. It is kept in
  memory only and handed to the game on your PC when it starts. It is never
  written to disk or to a log by the launcher.
- **Your Minecraft player name, player ID and skin**, to show who is signed in.
- **A local log** of what the launcher did (downloads, errors). It can contain
  your player name. It never contains tokens. It stays in
  `%LOCALAPPDATA%\com.meca.launcher\logs`.

## What leaves your PC

The launcher connects only to:

- Microsoft, Xbox Live and Minecraft Services, to sign you in and check that
  your account owns the game.
- Mojang's download servers, Forge's download server and Modrinth, to download
  game files and mods.
- GitHub, to read the modpack list in this repository.
- `textures.minecraft.net`, to show your skin.
- The Meca game server, to show whether it is online.

It sends nothing to the launcher's author or to any other third party. There
is no telemetry, no analytics and no advertising.

## Removing your data

- **Sign out** in the launcher deletes the stored renewal token.
- Deleting the folder `%LOCALAPPDATA%\com.meca.launcher` removes everything
  the launcher installed, including its logs.

## Contact

Open an issue in this repository.
