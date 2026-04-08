#!/usr/bin/env bash
# Migrate workspace files from old per-conversation PVCs (prism-ws-*)
# into the new shared PVC (prism-workspaces).
#
# For each old PVC, creates a temporary pod that mounts both the old PVC
# and the shared PVC, copies files, then deletes the pod and the old PVC.

set -euo pipefail

NAMESPACE="${NAMESPACE:-prism}"
SHARED_PVC="prism-workspaces"

echo "==> Listing old workspace PVCs in namespace ${NAMESPACE}..."
OLD_PVCS=$(kubectl get pvc -n "$NAMESPACE" -l app=prism-agent-workspace \
  -o jsonpath='{range .items[*]}{.metadata.name}{"\t"}{.metadata.labels.prism\.canonical\.com/session}{"\n"}{end}')

if [ -z "$OLD_PVCS" ]; then
  echo "No old workspace PVCs found. Nothing to migrate."
  exit 0
fi

echo "$OLD_PVCS" | while IFS=$'\t' read -r PVC_NAME SESSION_ID; do
  [ -z "$PVC_NAME" ] && continue

  echo ""
  echo "==> Migrating ${PVC_NAME} (session: ${SESSION_ID})..."

  POD_NAME="migrate-${PVC_NAME}"

  # Delete any leftover migration pod from a previous run.
  kubectl delete pod -n "$NAMESPACE" "$POD_NAME" --ignore-not-found --wait=false 2>/dev/null || true

  # Create a pod that mounts both PVCs and copies files.
  cat <<EOF | kubectl apply -n "$NAMESPACE" -f -
apiVersion: v1
kind: Pod
metadata:
  name: ${POD_NAME}
spec:
  restartPolicy: Never
  containers:
    - name: migrate
      image: busybox:latest
      command:
        - sh
        - -c
        - |
          echo "Copying files from /old to /shared/${SESSION_ID}/"
          mkdir -p "/shared/${SESSION_ID}"
          if [ -d /old ] && [ "\$(ls -A /old 2>/dev/null)" ]; then
            cp -a /old/. "/shared/${SESSION_ID}/"
            echo "Done. Files copied:"
            ls -la "/shared/${SESSION_ID}/"
          else
            echo "No files found in old workspace."
          fi
      volumeMounts:
        - name: old-workspace
          mountPath: /old
          readOnly: true
        - name: shared-workspace
          mountPath: /shared
  volumes:
    - name: old-workspace
      persistentVolumeClaim:
        claimName: ${PVC_NAME}
        readOnly: true
    - name: shared-workspace
      persistentVolumeClaim:
        claimName: ${SHARED_PVC}
EOF

  echo "    Waiting for migration pod to complete..."
  kubectl wait -n "$NAMESPACE" pod/"$POD_NAME" --for=condition=Ready --timeout=30s 2>/dev/null || true
  kubectl wait -n "$NAMESPACE" pod/"$POD_NAME" --for=jsonpath='{.status.phase}'=Succeeded --timeout=60s 2>/dev/null || true

  # Show logs.
  echo "    --- Pod logs ---"
  kubectl logs -n "$NAMESPACE" "$POD_NAME" 2>/dev/null || echo "    (no logs)"
  echo "    ----------------"

  # Clean up the migration pod.
  kubectl delete pod -n "$NAMESPACE" "$POD_NAME" --wait=false 2>/dev/null || true

  # Delete the old PVC.
  echo "    Deleting old PVC ${PVC_NAME}..."
  kubectl delete pvc -n "$NAMESPACE" "$PVC_NAME" 2>/dev/null || true

  echo "    Done migrating ${PVC_NAME}."
done

echo ""
echo "==> Migration complete."
