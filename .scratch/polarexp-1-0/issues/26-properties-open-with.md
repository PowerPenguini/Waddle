# 26: Properties, permissions, and Open With

Type: feature

**What to build:** Inspect and change ordinary file metadata and application associations without root mode.

**Blocked by:** None (can start immediately).

**Status:** resolved

- [x] Properties shows type, location, size, dates, permissions, and default application.
- [x] Users can change permissions allowed by their account.
- [x] Open With launches one chosen application.
- [x] Default-application selection changes later activation.

## Answer

Properties now opens in the existing bottom output surface and reports the selected entry's type,
MIME type, location, size, timestamps, symbolic and octal permissions, owner, default application,
and every compatible desktop application ID. `:chmod MODE` applies an octal mode to the current
selection with per-entry failure reporting. `:open-with APP_ID` launches the selected entry once,
while `:default-app APP_ID` updates the desktop MIME association used by later activation. These
operations run with the current account only; PolarExp neither elevates privileges nor exposes ACL
or extended-attribute editing in 1.0.
