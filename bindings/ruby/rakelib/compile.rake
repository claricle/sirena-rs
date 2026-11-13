# frozen_string_literal: true

require "rb_sys/toolchain_info"
require "rubygems/package"

# The gem builds exactly the way rb-sys-dock drives it — `rake
# native:$RUBY_TARGET gem` inside the cross container (see
# oxidize-rb/rb-sys gem/exe/rb-sys-dock). Each native:<platform> task
# vendors the etassia-ffi cdylib for that triple into
# lib/etassia/native/ inside the rake process — triple from rb_sys's
# own platform table, cargo build, cp, then declare the file on the
# spec and package it, like parsanol-ruby's cross_compiling callback.
# No workflow env or shell splicing.
PLATFORM_GEMS = %w[
  x86_64-linux
  aarch64-linux
  x86_64-darwin
  arm64-darwin
  x64-mingw-ucrt
].freeze

RUST_ROOT = File.expand_path("../../..", __dir__) # the rust workspace, three levels above rakelib/

def vendor_cdylib(plat)
  triple = RbSys::ToolchainInfo::DATA.fetch(plat).fetch("rust-target")
  target_dir = ENV["CARGO_TARGET_DIR"] || File.join(RUST_ROOT, "target")

  sh "cargo", "build", "--release", "-p", "etassia-ffi", "--target", triple, chdir: RUST_ROOT
  art = Dir[File.join(target_dir, "#{triple}/release/libetassia_ffi.{so,dylib}"),
            File.join(target_dir, "#{triple}/release/etassia_ffi.dll")].first
  raise "cdylib artifact not found for #{plat} (#{triple})" unless art

  mkdir_p "lib/etassia/native"
  cp art, "lib/etassia/native/"
  File.join("lib/etassia/native", File.basename(art))
end

namespace :native do
  PLATFORM_GEMS.each do |plat|
    desc "Vendor the #{plat} cdylib and build the #{plat} platform gem"
    task plat do
      vendored = vendor_cdylib(plat)

      spec = Gem::Specification.load("etassia-native.gemspec")
      raise "etassia-native.gemspec failed to load" if spec.nil?

      spec.platform = plat
      spec.files << vendored
      gem_file = Gem::Package.build(spec)
      mkdir_p "pkg"
      mv gem_file, "pkg/"
      puts "Built pkg/#{File.basename(gem_file)}"
    end
  end
end
