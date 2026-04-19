#!/usr/bin/env bash
# Test stryke LSP server manually via stdio.
# Usage: ./scripts/test-lsp.sh

set -euo pipefail

send_lsp() {
  local body="$1"
  printf 'Content-Length: %d\r\n\r\n%s' "${#body}" "$body"
}

# Sample Perl document for testing
DOC='sub greet { my $name = shift; say "Hello, $name!"; }
greet("world");
my $x = 42;
my @nums = (1, 2, 3);
'
DOC_ESCAPED=$(printf '%s' "$DOC" | jq -Rs .)

{
  # 1. Initialize
  send_lsp '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}'
  sleep 0.3

  # 2. Initialized notification
  send_lsp '{"jsonrpc":"2.0","method":"initialized","params":{}}'
  sleep 0.1

  # 3. Open a document (triggers diagnostics)
  send_lsp "{\"jsonrpc\":\"2.0\",\"method\":\"textDocument/didOpen\",\"params\":{\"textDocument\":{\"uri\":\"file:///tmp/test.pl\",\"languageId\":\"perl\",\"version\":1,\"text\":$DOC_ESCAPED}}}"
  sleep 0.3

  # 4. Request hover on "greet" (line 0, col 4)
  send_lsp '{"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{"textDocument":{"uri":"file:///tmp/test.pl"},"position":{"line":0,"character":4}}}'
  sleep 0.2

  # 5. Request completion after "$" (line 2, col 4)
  send_lsp '{"jsonrpc":"2.0","id":3,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///tmp/test.pl"},"position":{"line":2,"character":4}}}'
  sleep 0.2

  # 6. Go to definition of "greet" (line 1, col 0)
  send_lsp '{"jsonrpc":"2.0","id":4,"method":"textDocument/definition","params":{"textDocument":{"uri":"file:///tmp/test.pl"},"position":{"line":1,"character":0}}}'
  sleep 0.2

  # 7. Find references to "greet"
  send_lsp '{"jsonrpc":"2.0","id":5,"method":"textDocument/references","params":{"textDocument":{"uri":"file:///tmp/test.pl"},"position":{"line":0,"character":4},"context":{"includeDeclaration":true}}}'
  sleep 0.2

  # 8. Document symbols
  send_lsp '{"jsonrpc":"2.0","id":6,"method":"textDocument/documentSymbol","params":{"textDocument":{"uri":"file:///tmp/test.pl"}}}'
  sleep 0.2

  # 9. Document highlight on "$name" (line 0, col 18)
  send_lsp '{"jsonrpc":"2.0","id":7,"method":"textDocument/documentHighlight","params":{"textDocument":{"uri":"file:///tmp/test.pl"},"position":{"line":0,"character":18}}}'
  sleep 0.2

  # 10. Prepare rename on "greet" (line 0, col 4)
  send_lsp '{"jsonrpc":"2.0","id":8,"method":"textDocument/prepareRename","params":{"textDocument":{"uri":"file:///tmp/test.pl"},"position":{"line":0,"character":4}}}'
  sleep 0.2

  # 11. Rename "greet" to "welcome"
  send_lsp '{"jsonrpc":"2.0","id":9,"method":"textDocument/rename","params":{"textDocument":{"uri":"file:///tmp/test.pl"},"position":{"line":0,"character":4},"newName":"welcome"}}'
  sleep 0.2

  # 12. Edit document (triggers new diagnostics)
  send_lsp "{\"jsonrpc\":\"2.0\",\"method\":\"textDocument/didChange\",\"params\":{\"textDocument\":{\"uri\":\"file:///tmp/test.pl\",\"version\":2},\"contentChanges\":[{\"text\":\"sub broken { \\n\"}]}}"
  sleep 0.3

  # 13. Close document
  send_lsp '{"jsonrpc":"2.0","method":"textDocument/didClose","params":{"textDocument":{"uri":"file:///tmp/test.pl"}}}'
  sleep 0.1

  # 14. Shutdown
  send_lsp '{"jsonrpc":"2.0","id":10,"method":"shutdown"}'
  sleep 0.1

  # 15. Exit
  send_lsp '{"jsonrpc":"2.0","method":"exit"}'
} | timeout 10 stryke --lsp 2>&1 | sed 's/Content-Length: [0-9]*/\n/g' | grep -v '^$' | while IFS= read -r line; do
  echo "$line" | jq . 2>/dev/null || echo "$line"
done || true

echo ""
echo "LSP test completed."
