cat > Dockerfile.tmp <<EOF
FROM quay.io/podman/stable
RUN dnf -y install bindfs
EOF

podman build -t bindfs:latest -f Dockerfile.tmp .

podman run --user podman --device /dev/fuse -it bindfs:latest bash -ex -c "
    cd ~ && mkdir test1 test2 ;
    printf 'set -ex ; bindfs --no-allow-other test1 test2 ; echo bindfs OK\n' > test-fuse.sh
    unshare -pfr --user --mount --kill-child /bin/bash ./test-fuse.sh ;
    echo done"

rm Dockerfile.tmp
