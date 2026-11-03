# Headless-sway testbed for the exact-fractional-buffers acceptance check
# (test/verify-wayland-fractional.sh). Arch Linux matches the dev host's
# glibc: the pixpapers binary is BUILT ON THE HOST and mounted in, so no
# Rust toolchain lives in this image. sway >= 1.11 exposes both
# wp_viewporter and wp_fractional_scale_v1 — the pair the exact path needs.
FROM archlinux/archlinux:latest
RUN pacman -Syu --noconfirm sway libcap \
    # Arch ships sway with cap_sys_nice=ep (RT priority). Docker's default
    # no-new-privileges then refuses to exec it (EPERM). A wallpaper testbed
    # has no use for RT scheduling: strip the file capability.
    && setcap -r /usr/bin/sway \
    && pacman -Scc --noconfirm
