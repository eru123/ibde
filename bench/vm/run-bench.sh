#!/usr/bin/env bash
# Boot the floor-profile bench VM: 1 vCPU, 512 MB, software rendering.
# Seed delivered via NoCloud net datasource (host http.server on 10.0.2.2).
set -euo pipefail
cd "$(dirname "$0")"

IMG=debian-13-generic-amd64.qcow2
[[ -f "$IMG" ]] || ./fetch-image.sh

python3 -m http.server 8123 --bind 127.0.0.1 >/dev/null 2>&1 &
HTTP_PID=$!
trap 'kill $HTTP_PID 2>/dev/null || true' EXIT

rm -f serial.log qemu.pid
ACCEL=kvm; [[ -w /dev/kvm ]] || ACCEL=tcg

qemu-system-x86_64 \
  -machine pc,accel=$ACCEL \
  -cpu max \
  -smp 1 -m 512 \
  -drive file="$IMG",if=virtio,format=qcow2,cache=writeback \
  -device virtio-net-pci,netdev=net0 \
  -netdev user,id=net0,hostfwd=tcp:127.0.0.1:2222-:22 \
  -device virtio-vga \
  -display none \
  -serial file:serial.log \
  -smbios type=1,serial=ds=nocloud-net;s=http://10.0.2.2:8123/ \
  -no-reboot \
  -daemonize -pidfile qemu.pid

echo "bench VM booting (accel=$ACCEL) — serial -> $(pwd)/serial.log"
echo -n "waiting for measurement + poweroff"
while kill -0 "$(cat qemu.pid)" 2>/dev/null; do
  echo -n "."; sleep 5
done
echo " done."
./parse.sh
