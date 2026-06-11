#!/usr/bin/env bash
set -e

REPO_DIR="$(cd "$(dirname "$0")" && pwd)"
BINARY="$REPO_DIR/target/release/lyrics-on-screen"
HOST_NAME="com.vitormakino.lyrics_on_screen"

if [[ $# -lt 1 ]]; then
  echo "Uso: $0 <EXTENSION_ID>"
  echo ""
  echo "Como encontrar o ID da extensão:"
  echo "  1. Abra chrome://extensions"
  echo "  2. Ative o 'Modo de desenvolvedor' (canto superior direito)"
  echo "  3. Clique em 'Carregar sem compactação' e selecione a pasta 'extension/'"
  echo "  4. Copie o ID exibido abaixo do nome da extensão"
  echo ""
  echo "Exemplo: $0 abcdefghijklmnopabcdefghijklmnop"
  exit 1
fi

EXTENSION_ID="$1"

if [[ ! -f "$BINARY" ]]; then
  echo "Binário não encontrado: $BINARY"
  echo "Execute primeiro: cargo build --release"
  exit 1
fi

MANIFEST="{
  \"name\": \"$HOST_NAME\",
  \"description\": \"Lyrics on Screen native messaging host\",
  \"path\": \"$BINARY\",
  \"type\": \"stdio\",
  \"allowed_origins\": [\"chrome-extension://$EXTENSION_ID/\"]
}"

install_for() {
  local dir="$1"
  mkdir -p "$dir"
  echo "$MANIFEST" > "$dir/$HOST_NAME.json"
  echo "  instalado em: $dir/$HOST_NAME.json"
}

echo "Instalando native messaging host..."
install_for "$HOME/.config/google-chrome/NativeMessagingHosts"
install_for "$HOME/.config/chromium/NativeMessagingHosts"

echo ""
echo "Pronto! Recarregue a extensão em chrome://extensions para ativar."
