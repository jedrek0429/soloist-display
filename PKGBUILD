# Maintainer: Soloist Display contributors
pkgname=soloist-display-git
pkgver=0.1.0
pkgrel=1
pkgdesc='Spotify Soloist now-playing display and PipeWire visualiser'
arch=('x86_64')
url='https://github.com/jedrek0429/soloist-display'
license=('MIT')
depends=('xdg-utils' 'libpulse' 'pipewire-pulse' 'systemd' 'curl')
makedepends=('cargo' 'git')
provides=('soloist-display')
conflicts=('soloist-display')
source=("soloist-display::git+${url}.git#branch=main")
sha256sums=('SKIP')
build(){
  cd "$srcdir/soloist-display"
  cargo build --release --manifest-path audio-bridge/Cargo.toml
}
package(){
  cd "$srcdir/soloist-display"
  install -Dm755 audio-bridge/target/release/soloist-audio-bridge "$pkgdir/usr/bin/soloist-audio-bridge"
  install -Dm755 packaging/soloist-display-launcher "$pkgdir/usr/bin/soloist-display"
  install -Dm644 packaging/soloist-display.service "$pkgdir/usr/lib/systemd/user/soloist-display.service"
  install -Dm644 packaging/soloist-display.desktop "$pkgdir/usr/share/applications/soloist-display.desktop"
  install -Dm644 packaging/soloist-display-settings.desktop "$pkgdir/usr/share/applications/soloist-display-settings.desktop"
}
