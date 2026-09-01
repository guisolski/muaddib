class Muaddib < Formula
  desc "AI-powered meta-search for your terminal"
  homepage "https://github.com/guisolski/muaddib"
  version "0.2.4"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/guisolski/muaddib/releases/download/v0.2.4/muaddib-aarch64-apple-darwin.tar.gz"
      sha256 "6f0d150f58fb37b676909bb4a4384020447472878abd2069161295cc123841b1"
    else
      url "https://github.com/guisolski/muaddib/releases/download/v0.2.4/muaddib-x86_64-apple-darwin.tar.gz"
      sha256 "176bd6cd830256faa3fcb70358ceea83d896b5c068963672f7493509050ab70a"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/guisolski/muaddib/releases/download/v0.2.4/muaddib-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "9efd84a67f5b009bbcf26691e8d19e97aeddeaf85d334519afa9079e5d6df3b7"
    else
      url "https://github.com/guisolski/muaddib/releases/download/v0.2.4/muaddib-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "edb98f31e0b65d77b245807085da429b172eea751ec519a89fba33c3bd6b04c6"
    end
  end

  def install
    bin.install "muaddib"
  end

  test do
    assert_match "muaddib", shell_output("#{bin}/muaddib --version")
  end
end
