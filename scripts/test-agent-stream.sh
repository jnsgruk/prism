#!/usr/bin/env bash
# Test the agent container OpenCode flow directly, bypassing gRPC.
set -euo pipefail

NAMESPACE="prism"
AGENT_POD=$(kubectl get pod -n "$NAMESPACE" -l app=prism-agent \
  --field-selector=status.phase=Running \
  -o jsonpath='{.items[0].metadata.name}' 2>/dev/null || true)

if [ -z "$AGENT_POD" ]; then
  echo "No running agent pod found."
  exit 1
fi

echo "=== Agent pod: $AGENT_POD ==="

# Create a session
echo "--- Creating session ---"
SESSION_ID=$(kubectl exec -n "$NAMESPACE" "$AGENT_POD" -- \
  curl -s -X POST "http://localhost:4096/session" \
  -H "Content-Type: application/json" \
  -H "x-opencode-directory: /workspace" \
  -d '{"title":"test-stream"}' | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")
echo "Session: $SESSION_ID"

# Start SSE listener in background
echo "--- Starting SSE listener ---"
kubectl exec -n "$NAMESPACE" "$AGENT_POD" -- \
  curl -sN -H "x-opencode-directory: /workspace" "http://localhost:4096/event" \
  >/tmp/sse-events.txt 2>&1 &
SSE_PID=$!
sleep 1

# Send prompt via the prompt_async endpoint (same as SDK)
echo "--- Sending prompt ---"
RESP=$(kubectl exec -n "$NAMESPACE" "$AGENT_POD" -- \
  curl -s -w "\n%{http_code}" -X POST \
  "http://localhost:4096/session/$SESSION_ID/prompt_async" \
  -H "Content-Type: application/json" \
  -H "x-opencode-directory: /workspace" \
  -d '{"parts":[{"type":"text","text":"What is 2+2? Answer in one word."}]}')
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | head -n -1)
echo "HTTP $HTTP_CODE: $BODY"

if [ "$HTTP_CODE" -ge 400 ]; then
  echo "FAILED to send prompt"
  kill $SSE_PID 2>/dev/null || true
  exit 1
fi

# Wait for response
echo "--- Waiting 15s for events ---"
sleep 15
kill $SSE_PID 2>/dev/null || true
wait $SSE_PID 2>/dev/null || true

echo ""
echo "--- SSE events received ---"
EVENT_COUNT=$(grep -c "^data:" /tmp/sse-events.txt 2>/dev/null || echo 0)
echo "Total data frames: $EVENT_COUNT"

if [ "$EVENT_COUNT" -gt 0 ]; then
  echo ""
  echo "Event types:"
  grep "^data:" /tmp/sse-events.txt | while read -r line; do
    data="${line#data: }"
    python3 -c "
import json,sys
d = json.loads('''$data''')
t = d.get('type','?')
props = d.get('properties',{})
# For message.part.updated, show the part type
part_type = ''
if 'part' in props:
    part_type = props['part'].get('type','')
    if part_type == 'text':
        text = props['part'].get('text','')[:60]
        part_type = f'text: {text}'
    elif part_type == 'tool':
        tool = props['part'].get('tool','')
        part_type = f'tool: {tool}'
print(f'  {t} {part_type}')
" 2>/dev/null || echo "  (parse error)"
  done
else
  echo "NO EVENTS - this is the bug"
fi

# Check messages
echo ""
echo "--- Session messages ---"
kubectl exec -n "$NAMESPACE" "$AGENT_POD" -- \
  curl -s "http://localhost:4096/session/$SESSION_ID/message" \
  -H "x-opencode-directory: /workspace" | python3 -c "
import json,sys
msgs = json.load(sys.stdin)
for m in msgs:
    role = m['info']['role']
    err = m['info'].get('error')
    parts = []
    for p in m.get('parts',[]):
        if p.get('type') == 'text':
            parts.append(f'text:{p.get(\"text\",\"\")[:60]}')
        elif p.get('type') == 'tool':
            parts.append(f'tool:{p.get(\"tool\",\"\")}')
        else:
            parts.append(p.get('type','?'))
    print(f'  {role}: err={err} parts={parts}')
" 2>/dev/null || echo "  (parse error)"

echo ""
echo "=== Done ==="
