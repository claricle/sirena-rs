# frozen_string_literal: true

# Dumps the Ruby native parse tree of every .mmd under CORPUS_DIR as JSON.
#
#   ruby -I<sirena>/lib golden/dump_trees.rb <corpus_dir> <out_dir>
#
# corpus_dir holds <type>/<name>.mmd (the spec/mermaid layout, or golden/).
# One <out_dir>/<type>/<name>.json per case:
#   {"detected": corpus name | null, "entry": name | null, "tree": json | null}
# Slices are written as the parsanol wire object (value/line/column/offset/
# length); hash keys are strings. The Rust parity test reads these files.
require "json"
require "fileutils"
require "sirena"

ENTRY_FOR = { git_graph: "gitgraph" }.freeze

def wire(node)
  case node
  when Parsanol::Slice
    line, column = node.line_and_column
    { "value" => node.to_s, "line" => line, "column" => column,
      "offset" => node.offset, "length" => node.size }
  when Hash then node.to_h { |k, v| [k.to_s, wire(v)] }
  when Array then node.map { |v| wire(v) }
  else node
  end
end

corpus, out = ARGV
abort "usage: dump_trees.rb <corpus_dir> <out_dir>" unless corpus && out

NAMES = Sirena::CorpusTypeNames::DIRECTORIES
ARTIFACT = File.expand_path("lib/sirena/parser/native/mermaid.json",
                            Gem.loaded_specs["sirena"]&.full_gem_path ||
                            File.expand_path("../..", $LOADED_FEATURES.grep(/sirena\.rb\z/).first.to_s))
ENTRIES = JSON.parse(File.read(ARTIFACT))["entries"].keys.freeze

def detect(body)
  Sirena::Notation::Mermaid.detect_type(body)
rescue Sirena::Engine::DiagramTypeError
  nil
end

def dump_case(path, out)
  source = File.binread(path).force_encoding(Encoding::UTF_8)
  body = Sirena::Source.split(source)[:body]
  detected = detect(body)
  entry = detected && (ENTRY_FOR[detected] || detected.to_s)
  entry = nil unless ENTRIES.include?(entry)
  tree = nil
  if entry && body.valid_encoding?
    raw = Sirena::Parser::Native.parse(entry, body)
    tree = wire(raw) unless raw.nil?
  end
  target = File.join(out, File.basename(File.dirname(path)),
                     "#{File.basename(path, '.mmd')}.json")
  FileUtils.mkdir_p(File.dirname(target))
  File.write(target, JSON.generate(
    "detected" => detected && (NAMES.key(detected) || detected.to_s),
    "entry" => entry, "tree" => tree
  ))
end

Dir[File.join(corpus, "*", "*.mmd")].sort.each { |path| dump_case(path, out) }
