# MLCE: Linux containers on the guest's kernel. Expects a running machine called VM
# with an open network; ENGINES and REFERENCE select what is tested.
set -eu
. /root/meta/tests/vm/lib.sh
VM=${VM:-deb}
ENGINES=${ENGINES:-podman docker containerd}
REFERENCE=${REFERENCE:-docker.io/library/alpine:3.22}
install -m 755 "$BIN/meta-linux" /usr/local/bin/
container() { su -m tester -c 'exec meta-linux container "$@"' meta-linux "$@" </dev/null; }

for engine in $ENGINES; do
	container install --vm "$VM" --engine "$engine" >"/tmp/mlce-$engine.log" 2>&1 || {
		tail -20 "/tmp/mlce-$engine.log"
		fail "installing $engine"
	}
	ok "$engine is installed from the distribution"
	container run --vm "$VM" --engine "$engine" "$REFERENCE" cat /etc/os-release >"/tmp/mlce-$engine.log" 2>&1 || {
		tail -20 "/tmp/mlce-$engine.log"
		fail "running $engine"
	}
	grep -q 'ID=alpine' "/tmp/mlce-$engine.log" || fail "$engine ran something else"
	set +e
	container run --vm "$VM" --engine "$engine" "$REFERENCE" sh -c 'exit 7'
	code=$?
	set -e
	[ $code -eq 7 ] || fail "$engine: exit code $code"
	ok "$engine runs an OCI image and passes its exit code on"
	container list --vm "$VM" --engine "$engine"
done
expect_failure "an option is not an image" 'invalid image' \
	su -m tester -c "meta-linux container run --vm $VM -- --privileged true"
su -m tester -c "meta-exec explain --runtime vm /bin/sh" >/dev/null 2>&1 || true
meta-linux --json environments | grep -E '"(podman|docker|containerd)"'
echo "PASS mlce-containers"
