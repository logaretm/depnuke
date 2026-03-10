class Depnuke < Formula
  desc "Nuke node_modules, lock files, and package manager caches"
  homepage "https://github.com/logaretm/depnuke"
  version "0.1.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/logaretm/depnuke/releases/download/v#{version}/depnuke-aarch64-apple-darwin.tar.gz"
      # sha256 "PLACEHOLDER"
    else
      url "https://github.com/logaretm/depnuke/releases/download/v#{version}/depnuke-x86_64-apple-darwin.tar.gz"
      # sha256 "PLACEHOLDER"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/logaretm/depnuke/releases/download/v#{version}/depnuke-aarch64-unknown-linux-gnu.tar.gz"
      # sha256 "PLACEHOLDER"
    else
      url "https://github.com/logaretm/depnuke/releases/download/v#{version}/depnuke-x86_64-unknown-linux-gnu.tar.gz"
      # sha256 "PLACEHOLDER"
    end
  end

  def install
    bin.install "depnuke"
  end

  test do
    assert_match "depnuke", shell_output("#{bin}/depnuke --help")
  end
end
