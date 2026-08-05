class Muaddib < Formula
  desc "AI-powered meta-search for your terminal"
  homepage "https://github.com/guisolski/muaddib"
  version "0.1.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/guisolski/muaddib/releases/download/v0.1.0/muaddib-aarch64-apple-darwin.tar.gz"
      sha256 "8393713720ad084b218ee0d63f880d5a7b5b72741df41e22d5a6e625989c5cfa"
    else
      url "https://github.com/guisolski/muaddib/releases/download/v0.1.0/muaddib-x86_64-apple-darwin.tar.gz"
      sha256 "6f36f0998ae2ba26b67feecca2195aac14108ed77155938fc1bbb936abcc3445"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/guisolski/muaddib/releases/download/v0.1.0/muaddib-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "6b7eb4c6cb68517029888ad7b10b066c5c93954b959310e7a0bf65c4eee33981"
    else
      url "https://github.com/guisolski/muaddib/releases/download/v0.1.0/muaddib-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "2f80cac57baed89339f63d511f164e80e617f1b526cbc47b4c0320b1e68bf49f"
    end
  end

  def install
    bin.install "muaddib"
  end

  test do
    assert_match "muaddib", shell_output("#{bin}/muaddib --version")
  end
end
