#!/usr/bin/env bash
set -e

# Tentacle Daemon One-Line Auto-Installer

PANEL_URL=""
NODE_KEY=""

while [[ "$#" -gt 0 ]]; do
    case $1 in
        --url) PANEL_URL="$2"; shift ;;
        --url=*) PANEL_URL="${1#*=}" ;;
        --key) NODE_KEY="$2"; shift ;;
        --key=*) NODE_KEY="${1#*=}" ;;
        *) echo "Unknown parameter passed: $1"; exit 1 ;;
    esac
    shift
done

if [ -z "$PANEL_URL" ] || [ -z "$NODE_KEY" ]; then
    echo "❌ Error: Missing required arguments."
    echo "Please provide both --url and --key."
    exit 1
fi

echo "🐙 Starting Strixnodes Octopus Tentacle Daemon Setup..."

if [ "$EUID" -ne 0 ]; then
    echo "⚠️ Please run as root (sudo)."
    exit 1
fi

ARCH="x86_64"
DOWNLOAD_URL="https://github.com/OctopusPanel/tentacle/releases/latest/download/tentacle-deamon-linux-${ARCH}.tar.gz"

echo "📥 Downloading latest Tentacle Daemon release from GitHub..."
TMP_DIR=$(mktemp -d)
if ! curl -sSL -f "$DOWNLOAD_URL" -o "$TMP_DIR/tentacle-deamon.tar.gz"; then
    echo "⚠️ Latest release not found via URL, falling back to tag v0.1.0..."
    DOWNLOAD_URL="https://github.com/OctopusPanel/tentacle/releases/download/v0.1.0/tentacle-deamon-linux-${ARCH}.tar.gz"
    curl -sSL "$DOWNLOAD_URL" -o "$TMP_DIR/tentacle-deamon.tar.gz"
fi

tar -xz -C /usr/local/bin -f "$TMP_DIR/tentacle-deamon.tar.gz" tentacle-deamon
chmod +x /usr/local/bin/tentacle-deamon
rm -rf "$TMP_DIR"

echo "⚙️ Configuring Tentacle Daemon..."
mkdir -p /etc/tentacle

cat <<EOF > /etc/tentacle/config.json
{
    "panel_url": "${PANEL_URL}",
    "node_key": "${NODE_KEY}"
}
EOF

chmod 600 /etc/tentacle/config.json

echo "🛡️ Creating Systemd Service..."
cat << 'EOF' > /etc/systemd/system/tentacle.service
[Unit]
Description=Octopus Panel Tentacle Daemon
After=network-online.target
Wants=network-online.target docker.service

[Service]
Type=simple
User=root
Environment="TENTACLE_CONFIG=/etc/tentacle/config.json"
ExecStart=/usr/local/bin/tentacle-deamon
Restart=always
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable --now tentacle.service
systemctl restart tentacle.service

echo "✅ Tentacle Daemon installation and restart complete!"
echo "🚀 Status check: systemctl status tentacle.service"
echo "📡 Check live telemetry logs with: journalctl -u tentacle.service -n 50 --no-pager"
