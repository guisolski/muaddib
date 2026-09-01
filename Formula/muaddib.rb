class Muaddib < Formula
  desc "AI-powered meta-search for your terminal"
  homepage "https://github.com/guisolski/muaddib"
  version "0.2.3"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/guisolski/muaddib/releases/download/v0.2.3/muaddib-aarch64-apple-darwin.tar.gz"
      sha256 "558c463ad6de963b2626a038dd1ece814b3e849c64e0c87be974e0205fd601db"
    else
      url "https://github.com/guisolski/muaddib/releases/download/v0.2.3/muaddib-x86_64-apple-darwin.tar.gz"
      sha256 "0d64e798e13c07bb1db14bf4ed396ef88aee18f51789cb91fc6e8c2d530f4d1b"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/guisolski/muaddib/releases/download/v0.2.3/muaddib-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "4d11bfb209cfb4532df02e8348c6f9e0c6a99a21883e60ba6fa52a4833de6353"
    else
      url "https://github.com/guisolski/muaddib/releases/download/v0.2.3/muaddib-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "68855b61a9e1507fc0b756190a02baa4a03c67df4b00115dbeebf1ec0ee42511"
    end
  end

  def install
    bin.install "muaddib"
  end

  test do
    assert_match "muaddib", shell_output("#{bin}/muaddib --version")
  end
end
