# frozen_string_literal: true

# Records every Elkrb.layout call sirena makes while rendering the flowchart,
# state_diagram and class_diagram golden cases.
#
#   cd $SIRENA && bundle exec ruby /path/to/sirena-rs/golden/elk_dump.rb \
#     <golden_dir> <out_dir>
#
# One <out_dir>/<type>/<case>.json per case:
#   {"calls": [{"input": <graph Hash handed to Elkrb.layout>,
#               "output": <what sirena reads back>}]}
# output = {"id","x","y","width","height","children":[...],
#           "edges": [{"id","sections":[{"startPoint","endPoint","bendPoints"}]}]}
# Floats are JSON numbers (Ruby shortest round-trip); read them with exact
# float parsing (serde_json float_roundtrip).
require "bundler/setup"
require "date"
require "fileutils"
require "json"
require "timeout"

$LOAD_PATH.unshift(File.join(Dir.pwd, "lib"))
require "sirena"

TYPES = %w[flowchart state_diagram class_diagram].freeze
golden, out = ARGV
abort "usage: elk_dump.rb <golden_dir> <out_dir>" unless golden && out

def pt(point)
  { "x" => point.x, "y" => point.y }
end

def project(node)
  h = { "id" => node.id, "x" => node.x, "y" => node.y,
        "width" => node.width, "height" => node.height }
  h["children"] = (node.children || []).map { |c| project(c) }
  h["edges"] = (node.edges || []).map { |e| project_edge(e) }
  h
end

def project_edge(edge)
  { "id" => edge.id,
    "sections" => (edge.sections || []).map do |s|
      { "startPoint" => pt(s.start_point), "endPoint" => pt(s.end_point),
        "bendPoints" => (s.bend_points || []).map { |p| pt(p) } }
    end }
end

$calls = []
Elkrb.singleton_class.prepend(Module.new do
  def layout(input, *rest, **kw)
    snapshot = JSON.parse(JSON.generate(input))
    placed = super
    $calls << { "input" => snapshot, "output" => project(placed) }
    placed
  end
end)

total = 0
TYPES.each do |type|
  Dir.glob(File.join(golden, type, "*.mmd")).sort.each do |path|
    $calls = []
    begin
      Timeout.timeout(10) do
        Sirena::Engine.new.render(File.binread(path), today: Date.new(2026, 1, 1))
      end
    rescue StandardError, SystemStackError
      next
    end
    next if $calls.empty?

    dest = File.join(out, type, "#{File.basename(path, '.mmd')}.json")
    FileUtils.mkdir_p(File.dirname(dest))
    File.write(dest, JSON.generate({ "calls" => $calls }))
    total += 1
  end
end
puts "elk golden: #{total} cases -> #{out}"
