class Muaddib < Formula
  desc "AI-powered meta-search for your terminal"
  homepage "https://github.com/guisolski/muaddib"
  version "0.2.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/guisolski/muaddib/releases/download/v0.2.0/muaddib-aarch64-apple-darwin.tar.gz"
      sha256 "4bb8487489ce6cef6ad8d28f5c9a43725c8eb87ed89cb9213795f1dc98b5a281"
    else
      url "https://github.com/guisolski/muaddib/releases/download/v0.2.0/muaddib-x86_64-apple-darwin.tar.gz"
      sha256 "d6db4eb8ee248069183d674b2edaa92759acf3c01a96081e461695e9d064228b"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/guisolski/muaddib/releases/download/v0.2.0/muaddib-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "1c7c89720ea9e51e8484c62149958a3a29bf0d412561be620bb5db922363ccba"
    else
      url "https://github.com/guisolski/muaddib/releases/download/v0.2.0/muaddib-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "98cff24ee2c2fe067e59464058527b5c8d20ee7296a02a6f4536ededd4334ac2"
    end
  end

  def install
    bin.install "muaddib"
  end

  test do
    assert_match "muaddib", shell_output("#{bin}/muaddib --version")
  end
end
