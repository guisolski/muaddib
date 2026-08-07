class Muaddib < Formula
  desc "AI-powered meta-search for your terminal"
  homepage "https://github.com/guisolski/muaddib"
  version "0.2.1"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/guisolski/muaddib/releases/download/v0.2.1/muaddib-aarch64-apple-darwin.tar.gz"
      sha256 "0e75c8a951b264e7bf9504916425a62ee60fe0ae7eff0c2b209e787f0470c0b9"
    else
      url "https://github.com/guisolski/muaddib/releases/download/v0.2.1/muaddib-x86_64-apple-darwin.tar.gz"
      sha256 "458eb555e8cde759931dc00dd1afeec609b921bb85430901dfcd3902b8c04c33"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/guisolski/muaddib/releases/download/v0.2.1/muaddib-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "462c8dbad336be5dbe0f80d33bddcf54730c5a92f147a68d546b558a63e2a03b"
    else
      url "https://github.com/guisolski/muaddib/releases/download/v0.2.1/muaddib-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "756a496f7fab5a815dcf72766ec76e548c7c79549c74c2090a7f57f86fe42bef"
    end
  end

  def install
    bin.install "muaddib"
  end

  test do
    assert_match "muaddib", shell_output("#{bin}/muaddib --version")
  end
end
