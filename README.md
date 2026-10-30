<div align="center">

<img src="assets/logo.svg" width="96" height="96" alt="">

# Lueur

**One app for all your smart bulbs, whatever the brand, with care in every detail.**

Switch them, dim them, pick their white or their colour, play a mood or an
effect, set a sleep timer, or let them follow what's on screen, from one
window or the tray.

[**Download the latest version**](https://github.com/Alegzandr/lueur/releases/latest)

*Windows · macOS · Linux, free and open source*

<br>

<picture>
  <source media="(prefers-color-scheme: light)" srcset="docs/screenshots/room-light.png">
  <img src="docs/screenshots/room-dark.png" width="820" alt="Lueur's room plan: a screen, the seat facing it, two lights behind the screen and a floor lamp across the room, each glowing with the part of the picture it mirrors.">
</picture>

</div>

---

## Why Lueur

Yeelight, Philips Hue, WiZ, LIFX, Govee, Nanoleaf and Smart Life / Tuya
bulbs sit side by side in one window, found on your network and driven over
it, with no cloud once a bulb is set up. Each light has its power,
brightness, white and colour, here or in the tray menu, and is called by
its name, never by its address.

Pick lights on the Lighting page, and what you choose next goes to them: a
mode, still (Natural light, which follows the time of day, Daylight,
Reading, Relax and Night light) or animated (Screen, Music, which plays the
track on your lights like a stage, Rainbow, Sunset, Ocean and Candle), a
brightness, a white or a colour. Save the whole room as a scene, say Film
with the TV light on Screen, the kitchen off and the desk on Night light,
and bring it back in one click. A sleep timer, behind the clock, fades
everything off at night after a preset or any number of minutes.

The small things are taken care of. Lueur corrects every light for the
colour of your walls. Quit it, and on most bulbs a light playing Rainbow or
Candle goes on playing it by itself. Run it on several computers, and they
share the bulbs instead of fighting over them. Remove a light by mistake,
and one click brings it back. Whatever plays, no light flashes more than
three times a second.

With **Screen**, the lights follow what's on screen, many times a second,
with every step from the screen to the bulb kept as short as Lueur can make
it ([how](#keeping-the-delay-down)). You sketch your room, place the screen and drop
each light where it really stands: a lamp right behind the screen mirrors
its own slice of the picture, one across the room glows softly with the
whole scene. On Windows it starts by itself when a game or a video goes
fullscreen, gives every light back what it was showing when it ends, and
reads HDR screens without washing them out
([details below](#4-follow-the-screen)).

Lueur only reads the screen, the way OBS or Discord do, so it is safe to run
next to anti-cheat ([details below](#competitive-games-and-anti-cheat)). It
is a ~10 MB tray app that installs without admin rights and keeps itself up
to date.

## A look around

<table>
  <tr>
    <td width="50%">
      <picture>
        <source media="(prefers-color-scheme: light)" srcset="docs/screenshots/lighting-light.png">
        <img src="docs/screenshots/lighting-dark.png" alt="The Lighting page: saved scenes and every light as a card with what it does, beside the modes, still and animated, each a photograph of the same room in its light, and the brightness of the lights picked.">
      </picture>
      <p><b>Lighting</b>: pick lights, give them a mode and a
      brightness, or bring back a scene you saved.</p>
    </td>
    <td width="50%">
      <picture>
        <source media="(prefers-color-scheme: light)" srcset="docs/screenshots/room-light.png">
        <img src="docs/screenshots/room-dark.png" alt="The Room page: a floor plan with the screen, the seat and the lights.">
      </picture>
      <p><b>Room</b>: a floor plan you draw on. Drag the lights, the corners
      and the screen; the glows show what each light will mirror.</p>
    </td>
  </tr>
  <tr>
    <td width="50%">
      <picture>
        <source media="(prefers-color-scheme: light)" srcset="docs/screenshots/bulbs-light.png">
        <img src="docs/screenshots/bulbs-dark.png" alt="The Bulbs page: a bulb found on the network, and the lights in the room with their addresses.">
      </picture>
      <p><b>Bulbs</b>: Lueur finds the bulbs on your network. Identify one
      to see which is which, then give it a place in the room.</p>
    </td>
    <td width="50%">
      <picture>
        <source media="(prefers-color-scheme: light)" srcset="docs/screenshots/settings-light.png">
        <img src="docs/screenshots/settings-dark.png" alt="The Settings page: following fullscreen games and videos, language, appearance, startup and configuration export.">
      </picture>
      <p><b>Settings</b>: following fullscreen games, English or French,
      light or dark, start at login, and a backup of your setup.</p>
    </td>
  </tr>
</table>

## Get started

### 1. Install

Grab the file for your system from the
[latest release](https://github.com/Alegzandr/lueur/releases/latest):

| System | File | What to do |
|---|---|---|
| **Windows** | `Lueur_…_windows_x64_setup.exe` | Run it. Lueur installs itself for your account (no admin rights, no wizard) and opens. |
| **macOS** | `Lueur_…_macos_universal.zip` | Unzip it and move **Lueur** to *Applications*. |
| **Linux** | `Lueur_…_linux_x86_64.AppImage` | Make it executable (`chmod +x`) and run it. |

From then on Lueur **updates itself**: each launch checks for a new version,
verifies its signature and installs it before the app opens. It only ever
moves forward: an older version is refused, even one properly signed.

The release process can sign and notarize the Mac app once an Apple
developer certificate is set up, but the Mac app published so far is not
signed. The first launch needs your go-ahead in *System Settings → Privacy
& Security*, and after an update macOS may ask again for the microphone
(Music effect) and for the Keychain where Lueur keeps its key.

### 2. Add your bulbs

Open **Bulbs**: Lueur searches your network straight away. Add the bulbs you
want; **Identify** makes one flash so you know which is which.

Some bulbs need one thing first:

| Bulbs | Before searching |
|---|---|
| Yeelight, Govee | Turn on **LAN Control** in their phone app. |
| Philips Hue | Press the button on the bridge when Lueur asks. A bridge paired with Lueur 0.11 or older is offered to pair again: its lamps then stream, quick enough for the music. |
| Nanoleaf | Hold the controller's power button for 5 to 7 seconds when Lueur asks. |
| Smart Life / Tuya | Use **Smart Life** on the Bulbs page ([see below](#smart-life--tuya-bulbs)). |

### 3. Place them in the room

Open **Room**. Drag the **screen** to where your monitor is, drag **You** to
your seat to turn the screen towards you, and drag each **light** to where it
really stands. New bulbs start in a row behind the screen, the most common
setup.

Each light also has a **height** (floor to ceiling), which picks the part of
the picture it follows: a lamp on the floor mirrors the bottom of the screen,
one on a shelf the top.

### 4. Follow the screen

Back on **Lighting**, click the **Screen** tile (or press
<kbd>Ctrl</kbd>/<kbd>⌘</kbd> + <kbd>Enter</kbd>), then launch a game or a
film. On Windows you can skip this step: with *Follow fullscreen games and
videos* on (the default, in **Settings**) it starts and stops by itself when
a game or a video goes fullscreen on the screen it reads; a video fullscreen
on another screen leaves the lights alone.

The lights follow the shot, not every small movement in it: while a scene
stays the same and only someone or the camera moves a little, each bulb keeps
its colour, and only its brightness follows. A new shot changes it at once.
A grey or near-black picture gives a white of the matching warmth rather than
a faint tint, so a night scene does not turn the room lavender.

With HDR on in Windows, Lueur reads the colours as they are composed
and matches them to your *SDR content brightness* setting, so they don't come
out washed out the way an HDR screen often looks in screen sharing. There is
nothing to set: an *HDR* tag on the Screen tile shows when it is so.

When a film or video doesn't fill the screen (a wide film with bands above
and below it, a 16:9 video on an ultrawide screen), Lueur leaves the
black bars out after a couple of seconds, so the lights near those edges
follow the picture instead of going dark. Subtitles or a player's controls in
a bar don't bring it back. *Ignore black bars* in **Settings** turns this
off.

*Dark scenes*, in **Settings**, sets how much a dark picture lights the room:
*Lit* (the default) keeps some light on even in a night scene, *As on screen*
gives the room the same share of light the screen gives, and *Balanced* sits
between them. *Colour strength* sets how much the lights strengthen the
picture's colours, from *As on screen* to *Vivid*: bulbs tend to wash out a
screen's colours, so a little extra is on by default.

With *Photosensitivity protection* on (the default, in **Settings**), no
light flashes more than three times a second, and neither does the room when
lights take turns, like a siren flashing left and right. A sudden glare, such
as a flashbang, takes a tenth of a second to reach full brightness, and
longer when the picture keeps flashing.

If the screen can't be read for a while (screen sharing stopped on Linux, a
display gone), the Screen tile says so, and the lights keep their colour
until it's back. Films from apps that protect their picture, such as the
Netflix or Disney+ apps, come out black to every screen capture, Lueur's
included: the lights then stay dim.

## Supported bulbs

| Family | Status |
|---|---|
| Yeelight Color | ✅ tested |
| Smart Life / Tuya colour bulbs | ✅ tested (v3.3) |
| Philips Hue (through the bridge) | supported, not yet tested on hardware. White Ambiance lamps show the white nearest to each colour |
| WiZ | supported, not yet tested on hardware |
| LIFX | supported, not yet tested on hardware |
| Govee (LAN Control on) | supported, not yet tested on hardware |
| Nanoleaf Shapes, Canvas, Elements, Lines, Light Panels (up-to-date firmware) | supported, not yet tested on hardware. Not the Essentials bulbs and strips, which have no local API |

Everything is driven over your local network; no cloud is involved once a
bulb is set up. Tried one of the untested families? An
[issue](https://github.com/Alegzandr/lueur/issues) saying how it went helps a
lot.

### Smart Life / Tuya bulbs

Tuya bulbs only obey apps that know their *local key*, which the Smart Life
app keeps to itself. **Smart Life** on the Bulbs page fetches it once:

1. In the Smart Life app, open **Me → ⚙ → Account and Security → User Code**
   and type that code into Lueur.
2. Scan the QR code Lueur shows from the Smart Life app, and confirm.
3. Lueur lists the colour bulbs in your homes and keeps each one's id and
   local key, then signs out. No account token is stored; everything after
   that is local.

Bulbs someone shared with you one by one aren't visible this way: ask their
owner to add you to their home instead.

A Tuya bulb takes about ten colours a second, fewer than the other families.
In the music show and with Screen it changes colour as often as it can,
straight to each new colour rather than fading, so it keeps up with the
others in coarser steps.

## Everyday use

- **Tray.** Closing the window keeps Lueur in the tray. Turn off *Keep
  running when closed* in Settings and closing quits instead, while
  minimising goes to the tray (on Windows and Linux; a Mac minimises to the
  Dock). The tray menu says what the lights show, starts and stops Screen and
  the effects and applies a mood, so you never have to leave a game.
  Once an update is installed, it also restarts Lueur into it.
- **Sleep timer.** While it runs, Lueur stays open even when closing the
  window would quit it, and keeps the computer from going to sleep by itself
  (on Windows and macOS). Shut the computer down and the lights stay on.
- **Quitting.** A light playing Rainbow, Sunset, Ocean or Candle goes on
  playing it by itself until Lueur is back, the way its own app would: a
  Yeelight, a Tuya bulb or Nanoleaf panels play the show's colours in a
  loop, a WiZ bulb its matching scene (True colours, Sunset, Ocean,
  Candlelight), a LIFX bulb sways between two of the colours, and a Hue lamp
  plays its colour loop for Rainbow. The rest settle on a warm white: the
  screen and the music need Lueur to see and hear, and a Govee light
  takes no loop over the network.
- **At login.** *Open at login* and *Start hidden in the tray* in Settings;
  each light picks up the effect it was left playing.
- **Keyboard.** <kbd>Ctrl</kbd>/<kbd>⌘</kbd> + <kbd>1</kbd>–<kbd>4</kbd>
  switches page, <kbd>Ctrl</kbd>/<kbd>⌘</kbd> + <kbd>Enter</kbd> starts or
  stops. On the room plan, arrows nudge the selection (<kbd>Shift</kbd> for
  bigger steps), <kbd>Delete</kbd> removes it, <kbd>Esc</kbd> deselects, and
  holding <kbd>Shift</kbd> while turning the screen snaps to 15°.
- **Mistakes.** Removing a light can be undone, and the room plan has undo and
  redo. Changes made while effects play come with a one-click
  *Apply*.
- **Music.** On Windows the Music effect listens to the computer's own sound,
  so it follows any song or game: the default output, another one, or only
  what one application plays (Settings, *Sound source*), so a call or a
  notification stays out of the show. On macOS and Linux it listens to the
  microphone, or another input. It hears the kick, the snare and the
  hi-hats apart, finds the tempo (from the snares and the strums too, in a
  song without a kick), and sends each beat early by however long each
  bulb takes to show it, so every light lands with the sound together.
  With Bluetooth headphones or a TV's speakers, *Sound delay* in Settings
  holds the lights back as long as the sound takes to reach you. It plays
  the same whatever the volume, in Windows or in the player, while a quiet
  passage still reads as quiet. It plays like a lighting desk: the colours
  hold through each phrase and change as the next one lands, where the
  song's own phrases start, the room rests in one colour and the beats
  strike in another, from near dark, each hit fading out just before the
  next beat. Each light gets a part from where it stands: the quick ones
  near the screen punch the beat in patterns that travel across the room,
  from side to side or from the middle out (and along a strip, a gradient
  lightstrip or a Nanoleaf layout, segment by segment), slow bulbs and
  lamps out in the room wash it in colour, and a light at the ceiling
  answers the snare. A
  breakdown rests in one deep colour, a build's chase quickens and climbs
  to white, and the drop lands on every light at once, into new colours.
  White flashes can be turned off, and never come more than three a
  second.
- **Several computers.** Lueur can run on every computer in the house. Each
  one tells the others which bulbs it is playing an effect on, and leaves
  those alone: a light one computer drives shows *On* and that computer's
  name on the other, and waits there, picking up again as soon as the first
  one lets go. *Take over* moves it at once. Nothing Lueur starts by itself,
  like following a fullscreen game, ever takes a bulb from another
  computer, and the sleep timer only puts out the lights it drives. A
  Yeelight that another app streams to is left alone the same way.
- **Backup.** **Settings → Configuration** exports your setup to a file and
  imports it back, to keep a copy or move to another computer. The file holds
  no password or key, so it is safe to share; on another computer, pair the
  bridges and import Smart Life again.

## How the room mapping works

The room is a top-down floor plan, and the screen faces where you sit. For
each light Lueur reads three things off the plan:

- **Sideways position** picks the horizontal slice of the picture it mirrors.
- **Height** picks the vertical band.
- **Distance** widens that region: tucked behind the panel, a light mirrors a
  tight slice; out across the room, the whole picture. Far lights are also
  dimmer and calmer, because a far wall that flickers at 60 Hz reads as a
  fault.

A light with several segments uses them. A strip or a gradient lightstrip on
the back of the screen follows the rim of the picture, from the bottom of its
left side, across the top, to the bottom of its right side; one under the
screen follows the bottom of the picture across its whole width, and one
above it the top; one elsewhere in the room follows the part of the picture in front of the
stretch of wall it lights.

A Nanoleaf layout plays the way it hangs, turned as you turned it in the
Nanoleaf app: across, its panels mirror the picture from left to right; one
taller than it is wide mirrors it from the bottom up.

Neighbouring lights hand over gradually when something bright crosses between
them, and colours are mixed the way light mixes, so reds and blues stay vivid
instead of turning muddy.

## Keeping the delay down

A light that follows the screen or the music only convinces if it keeps up,
so Lueur shortens every step between the picture or the sound and the bulb:

- **Reading the screen.** On Windows the screen is read through Desktop
  Duplication, the API OBS uses, and the picture is shrunk on the graphics
  card before it reaches the processor; on Linux it is shrunk as it arrives.
  Each update takes the latest picture shown, and on a still screen Lueur
  waits for the next one instead of reading the same one again.
- **Reaching the bulbs.** Each family gets its quickest way in. Yeelight
  bulbs run in *music mode*, a direct connection with no rate limit. Hue
  lamps are streamed to through the bridge's Entertainment API rather than
  sent one command at a time, and Nanoleaf panels take a stream too. WiZ,
  LIFX and Govee get a single network packet per colour. A Tuya bulb, which
  takes about ten colours a second, goes straight to each new colour instead
  of fading.
- **No fade left behind.** A bulb's fade never lasts longer than the gap to
  the next colour, so a bulb never chases a colour it has no time to reach.
- **All together.** Bulbs don't answer at the same speed: a Yeelight shows a
  colour in a few hundredths of a second, a Hue lamp behind its bridge takes
  several times that. The quick ones wait for the slowest, so a cut on
  screen reaches the whole room at once instead of rippling across it. A
  lamp too slow to strike, such as a Hue lamp the bridge can't stream to, is
  not waited for: it would hold the whole room back.
- **Ahead of the beat.** A beat, unlike the next picture, can be seen
  coming. The Music effect sends each one early by however long each bulb
  takes to show it, so every light lands with the sound, and *Sound delay*
  covers headphones or speakers that are late themselves.

There is no measured figure yet: the delay you see depends on the bulbs, the
network and the computer.

## Competitive games and anti-cheat

Lueur is built to run next to Vanguard, Easy Anti-Cheat, BattlEye, FACEIT and
the like without looking like a cheat:

- **Desktop capture only.** It reads whole monitors with the same system APIs
  as OBS and Discord. It never captures a game window directly.
- **Hands off the game.** No access to the game process, no memory reads, no
  injection, no hooks, no driver. Even when the Music effect follows one
  application's sound, Windows' audio service does the listening: Lueur
  never opens that application.
- **Output only.** No simulated keyboard or mouse input and no global
  hotkeys. The only output is colour sent to bulbs on your network.
- **No overlay.** Nothing is ever drawn over the game.

If a game hides itself from capture, the bulbs simply see a black screen;
switching the game to *borderless windowed* usually fixes it. Lueur never
tries to get around that.

## Privacy

Lueur talks to your bulbs on your local network, and only at private
addresses. Beyond that it sends exactly two kinds of traffic: the update
check to GitHub over HTTPS, and Tuya's sign-in while you import Smart Life
bulbs. That sign-in identifies itself with the app id Home Assistant uses
with Tuya, as Lueur has none of its own yet.

While it plays an effect, Lueur also broadcasts to your local network, about
once a second, which bulbs it drives and the computer's name, so other
computers running Lueur leave them alone (UDP port 47474). Nothing secret
goes out, and nothing in what it hears ever reaches a bulb: a forged message
can at most make a light wait, which *Take over* always overrides.

On your network, a Hue bridge is reached over HTTPS, and only ever with
the certificate it showed the first time Lueur saw it (at pairing, or for
a bridge paired with an older version, at its usual address). Nothing
vouches for that first sight: Lueur trusts it, then holds the bridge to
it. A bridge that does not answer over HTTPS is not paired, except the
first, round bridge, which has no HTTPS. That one and Nanoleaf controllers
only speak plain HTTP, so their credentials cross your network
unencrypted, and only to the address they were paired at. A bridge or
controller that turns up at a new address without proving who it is gets
nothing: the Bulbs page offers to pair it again with its button, or, when
nothing is pinned for it, to confirm that it is yours. A Hue bridge that
shows another certificate than its own can only be paired again.
Yeelight, WiZ, LIFX and Govee bulbs take commands from anyone on the
network, by design. Tuya bulbs get commands encrypted with their local
key, which itself never crosses the network, sent to wherever the bulb
was last found.

Bulb and bridge credentials never leave the app, nor go into an exported
configuration. On disk they are encrypted, with a key kept in your system's
keyring (Credential Manager on Windows, the Keychain on macOS, the Secret
Service on Linux). Without a keyring, on a Linux session with no Secret
Service for instance, they stay unencrypted in the settings file, which only
your account can read, and Settings says so.

Settings live in a `lueur` folder in your user config folder (`%APPDATA%` on
Windows, `~/Library/Application Support` on macOS, `~/.config` on Linux).

## Uninstalling

- **Windows:** *Settings → Apps → Installed apps → Lueur → Uninstall*. This
  removes everything but your settings.
- **macOS / Linux:** delete the app or the AppImage.

To remove your settings too, delete the `lueur` folder listed under
[Privacy](#privacy).

## Roadmap

- WLED.
- Testing WiZ, LIFX, Govee, Hue and Nanoleaf on real hardware.
- Optional per-light manual zone override.
- Multi-monitor spanning and per-zone brightness curves.

## Contributing

Building from source, the architecture, the logs and the release process
are in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md). To add a bulb family,
see [docs/ADDING_A_FAMILY.md](docs/ADDING_A_FAMILY.md).

## License

MIT, see [LICENSE](LICENSE).
