class Muaddib < Formula
  desc "AI-powered meta-search for your terminal"
  homepage "https://github.com/guisolski/muaddib"
  version "0.2.2"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/guisolski/muaddib/releases/download/v0.2.2/muaddib-aarch64-apple-darwin.tar.gz"
      sha256 "f4d86cbc2bf4f52247d42ef9c6f4d15e44c18e3dcf151ceb629db24f21298adb"
    else
      url "https://github.com/guisolski/muaddib/releases/download/v0.2.2/muaddib-x86_64-apple-darwin.tar.gz"
      sha256 "8de6e4f805ec93aa5c830c703bd64f35dc0c5564193835c7ac8dd8f98ff6d2ae"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/guisolski/muaddib/releases/download/v0.2.2/muaddib-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "6177e1aac9dc0ad4694229c3a56001d9e91d201477f4f1d52e9710065d5aff11"
    else
      url "https://github.com/guisolski/muaddib/releases/download/v0.2.2/muaddib-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "f6a13be8d4732d43d286728c965863dbab736a940549611b6080ba463d162112"
    end
  end

  def install
    bin.install "muaddib"
  end

  test do
    assert_match "muaddib", shell_output("#{bin}/muaddib --version")
  end
end
