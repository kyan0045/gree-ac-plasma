#!/bin/sh
# Install (or upgrade) the Gree AC Plasma applet for the current user.
# Afterwards, right-click the panel -> "Add or manage widgets" -> "Gree AC".
set -eu
if ! command -v kpackagetool6 >/dev/null 2>&1; then
    echo "error: kpackagetool6 not found. Install Plasma 6 (kpackagetool6) and try again." >&2
    exit 1
fi
DIR="$(cd "$(dirname "$0")/com.greeac.ac" && pwd)"
if kpackagetool6 -t Plasma/Applet --upgrade "$DIR" 2>/dev/null; then
    echo "Upgraded the Gree AC widget."
else
    kpackagetool6 -t Plasma/Applet --install "$DIR"
    echo "Installed the Gree AC widget."
    echo "Add it with: right-click the panel -> \"Add or manage widgets\" -> \"Gree AC\"."
fi
