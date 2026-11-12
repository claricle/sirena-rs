# frozen_string_literal: true

# Generates golden/<type>/<case>.svg and golden/manifest.json from the Ruby
# Sirena path. Run from a sirena checkout (feat/mermaid-parg or later):
#
#   cd $SIRENA && bundle exec ruby /path/to/sirena-rs/scripts/golden_generate.rb
#
# Env: GOLDEN_OUT (default: <this repo>/golden), SIRENA_CORPUS (default:
# $PWD/spec/mermaid). Sources are copied next to each svg. Every spec/mermaid/*/*.mmd is rendered with today pinned
# to 2026-01-01; cases that raise or time out are listed under "errors".
require "bundler/setup"
require "date"
require "digest"
require "fileutils"
require "json"
require "timeout"

$LOAD_PATH.unshift(File.join(Dir.pwd, "lib"))
require "etassia"

OUT = ENV.fetch("GOLDEN_OUT", File.expand_path("../golden", __dir__))
CORPUS = ENV.fetch("SIRENA_CORPUS", File.join(Dir.pwd, "spec", "mermaid"))
TODAY = Date.new(2026, 1, 1)

FileUtils.rm_rf(OUT)
FileUtils.mkdir_p(OUT)
cases = []
errors = []
Dir.glob(File.join(CORPUS, "*", "*.mmd")).sort.each do |path|
  rel = path.delete_prefix("#{CORPUS}/")
  source = File.binread(path)
  begin
    svg = Timeout.timeout(10) { Etassia::Engine.new.render(source, today: TODAY) }
  rescue StandardError, SystemStackError => e
    errors << { "case" => rel, "error" => e.class.name }
    next
  end
  next unless svg.is_a?(String)

  dest = File.join(OUT, rel.sub(/\.mmd\z/, ".svg"))
  FileUtils.mkdir_p(File.dirname(dest))
  File.binwrite(dest, svg)
  File.binwrite(dest.sub(/\.svg\z/, ".mmd"), source)
  cases << { "case" => rel, "type" => rel.split("/").first,
             "source_sha256" => Digest::SHA256.hexdigest(source),
             "svg_sha256" => Digest::SHA256.hexdigest(svg.b) }
end
File.write(File.join(OUT, "manifest.json"),
           "#{JSON.pretty_generate({ "today" => TODAY.to_s, "cases" => cases,
                                     "errors" => errors })}\n")
puts "golden: #{cases.size} rendered, #{errors.size} errors -> #{OUT}"
