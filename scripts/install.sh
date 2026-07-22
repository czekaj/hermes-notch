#!/usr/bin/env bash
# Hermes Notch — host-side installer.
# Links the widget host plugin, built-in widgets, the widget spec, and the
# notch-widgetizer skill into an existing Hermes Agent home (~/.hermes), and
# enables the plugin in config.yaml. Idempotent; run it again after git pull.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HERMES="${HERMES_DIR:-$HOME/.hermes}"
CONFIG="$HERMES/config.yaml"

if [[ ! -d "$HERMES" ]]; then
  echo "error: $HERMES not found — install Hermes Agent first" >&2
  exit 1
fi

echo "Hermes Notch installer"
echo "  repo:   $REPO"
echo "  hermes: $HERMES"
echo

# 1. Widget host plugin → ~/.hermes/plugins/hermes-notch/dashboard
mkdir -p "$HERMES/plugins/hermes-notch"
DASH="$HERMES/plugins/hermes-notch/dashboard"
if [[ -d "$DASH" && ! -L "$DASH" ]]; then
  # a real directory here would swallow the symlink (ln would nest it inside)
  if [[ -z "$(ls -A "$DASH")" ]]; then
    rmdir "$DASH"
  else
    echo "error: $DASH exists and is not empty — move it aside and re-run" >&2
    exit 1
  fi
fi
ln -sfn "$REPO/hermes-plugin" "$DASH"
echo "✓ plugin linked: plugins/hermes-notch/dashboard → $REPO/hermes-plugin"

# 2. Built-in widgets → ~/.hermes/notch-widgets/<id>
mkdir -p "$HERMES/notch-widgets/.spec"
for w in "$REPO"/widgets/*/; do
  w="${w%/}"
  ln -sfn "$w" "$HERMES/notch-widgets/$(basename "$w")"
  echo "✓ widget linked: notch-widgets/$(basename "$w")"
done

# 3. Widget spec copy (referenced by the widgetizer skill and GET /spec)
cp "$REPO/docs/WIDGET_SPEC.md" "$HERMES/notch-widgets/.spec/WIDGET_SPEC.md"
echo "✓ spec installed: notch-widgets/.spec/WIDGET_SPEC.md"

# 4. notch-widgetizer skill → ~/.hermes/skills/notch-widgetizer
mkdir -p "$HERMES/skills"
ln -sfn "$REPO/skills/notch-widgetizer" "$HERMES/skills/notch-widgetizer"
echo "✓ skill linked: skills/notch-widgetizer"

# 5. Enable the plugin in config.yaml (targeted insert — never rewrites the file)
if [[ -f "$CONFIG" ]]; then
  if grep -qE '^\s*-\s*hermes-notch\s*$' "$CONFIG"; then
    echo "✓ plugin already enabled in config.yaml"
  elif grep -qE '^plugins:' "$CONFIG"; then
    awk '
      { print }
      /^plugins:/      { in_plugins = 1; next_enabled = 1 }
      in_plugins && /^  enabled:/ && next_enabled {
        print "  - hermes-notch"; next_enabled = 0
      }
    ' "$CONFIG" > "$CONFIG.notch-tmp"
    if grep -qE '^\s*-\s*hermes-notch\s*$' "$CONFIG.notch-tmp"; then
      mv "$CONFIG.notch-tmp" "$CONFIG"
      echo "✓ plugin enabled in config.yaml (plugins.enabled)"
    else
      rm -f "$CONFIG.notch-tmp"
      echo "! could not auto-enable — add 'hermes-notch' to plugins.enabled in $CONFIG"
    fi
  else
    printf '\nplugins:\n  enabled:\n  - hermes-notch\n  disabled: []\n' >> "$CONFIG"
    echo "✓ plugins section added to config.yaml"
  fi
else
  echo "! $CONFIG not found — add 'hermes-notch' to plugins.enabled manually"
fi

echo
echo "Next steps:"
echo "  1. Remote access needs dashboard auth (the server refuses public binds without it):"
echo "       configure dashboard.basic_auth (username / password_hash / secret) in config.yaml"
echo "  2. Start the host:   hermes serve --host 0.0.0.0 --port 9119"
echo "  3. Point the Hermes Notch app at this machine's address."
echo "  4. Validate widgets: python3 $REPO/hermes-plugin/validate_widget.py <widget-dir> --run"
