# frozen_string_literal: true

# Times Rust render (sirena-ffi cdylib, called through bindings/ruby) against
# Ruby render (the pure-Ruby Sirena engine in SIRENA_RUBY_ROOT) on every golden
# case, then writes benchmarks/RESULTS.md.
#
#   ruby benchmarks/bench.rb
#
# Env:
#   SIRENA_RUBY_ROOT  Ruby sirena checkout whose lib/ is loaded
#                     (default /private/tmp/hk-sirena-parg)
#   BENCH_ITERATIONS  timed renders per case (default 5); one untimed warm-up first
#   BENCH_OUT         markdown file to write (default benchmarks/RESULTS.md)
#
# Each side runs in its own child process: the Ruby checkout defines its own
# Sirena::Native, which would clash with bindings/ruby in one process.
# A case is timed only when both sides render it; the rest are counted as skipped.

require "date"
require "json"
require "rbconfig"
require "tempfile"

ROOT = File.expand_path("..", __dir__)
GOLDEN = File.join(ROOT, "golden")
RUBY_ROOT = ENV.fetch("SIRENA_RUBY_ROOT", "/private/tmp/hk-sirena-parg")
ITERATIONS = Integer(ENV.fetch("BENCH_ITERATIONS", "5"))
OUT = ENV.fetch("BENCH_OUT", File.join(__dir__, "RESULTS.md"))
TODAY = Date.new(2026, 1, 1) # same pin as scripts/golden_generate.rb

def case_paths
  Dir.glob(File.join(GOLDEN, "*", "*.mmd")).sort.map { |path| path.delete_prefix("#{GOLDEN}/") }
end

# Child-process side: render every case with one renderer and dump raw samples.
def rust_renderer
  require_relative "../bindings/ruby/lib/sirena/native"
  lib = Sirena::Native.library or abort "no libsirena_ffi under target/release (run cargo build --release -p sirena-ffi)"
  ->(source) { lib.render(source) }
end

def ruby_renderer
  $LOAD_PATH.unshift(File.join(RUBY_ROOT, "lib"))
  require "sirena"
  ->(source) { Sirena::Engine.new.render(source, today: TODAY) }
end

def measure(source, render)
  render.call(source) # untimed warm-up
  samples = Array.new(ITERATIONS) do
    started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    render.call(source)
    (Process.clock_gettime(Process::CLOCK_MONOTONIC) - started) * 1000.0
  end
  { "status" => "ok", "samples_ms" => samples }
rescue StandardError, SystemStackError => e
  if e.class.name == "Sirena::Native::Unported"
    { "status" => "unported", "message" => e.message[0, 120] }
  else
    { "status" => "error", "class" => e.respond_to?(:error_class) ? e.error_class : e.class.name }
  end
end

def run_worker(side, out)
  render = side == "rust" ? rust_renderer : ruby_renderer
  cases = case_paths.to_h do |rel|
    source = File.binread(File.join(GOLDEN, rel)).force_encoding(Encoding::UTF_8)
    [rel, measure(source, render)]
  end
  File.write(out, JSON.generate({ "side" => side, "cases" => cases }))
end

# Parent process: build, run both sides, summarize, write markdown.
def build_ffi
  ok = system("cargo", "build", "--release", "-p", "sirena-ffi", chdir: ROOT, out: File::NULL)
  abort "cargo build --release -p sirena-ffi failed" unless ok
end

def run_side(side)
  Tempfile.create(["bench-#{side}", ".json"]) do |file|
    file.close
    ok = system(RbConfig.ruby, __FILE__, "--worker", side, file.path, chdir: ROOT)
    abort "#{side} worker failed" unless ok
    JSON.parse(File.read(file.path))
  end
end

def median(values)
  sorted = values.sort
  mid = sorted.size / 2
  sorted.size.odd? ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2.0
end

def describe(side, result)
  case result["status"]
  when "ok" then nil
  when "unported" then "#{side} unported"
  else "#{side} error #{result["class"]}"
  end
end

def summarize(rust, ruby)
  per_type = Hash.new { |h, k| h[k] = { timed: 0, skipped: 0, rust_ms: 0.0, ruby_ms: 0.0 } }
  reasons = Hash.new(0)
  case_paths.each do |rel|
    type = rel.split("/").first
    r = rust["cases"].fetch(rel)
    b = ruby["cases"].fetch(rel)
    agg = per_type[type]
    skip = [describe("rust", r), describe("ruby", b)].compact
    if skip.empty?
      agg[:timed] += 1
      agg[:rust_ms] += median(r["samples_ms"])
      agg[:ruby_ms] += median(b["samples_ms"])
    else
      agg[:skipped] += 1
      skip.each { |reason| reasons[reason] += 1 }
    end
  end
  [per_type.sort.to_h, reasons.sort.to_h]
end

def command_output(cmd)
  out = `#{cmd} 2>/dev/null`.strip
  out.empty? ? "unknown" : out
end

def ruby_checkout_rev
  Dir.chdir(RUBY_ROOT) { command_output("git rev-parse --short HEAD") }
rescue SystemCallError
  "unknown"
end

def fmt(ms) = format("%.1f", ms)

def render_markdown(per_type, reasons, rust_count, ruby_count)
  total = per_type.values.each_with_object({ timed: 0, skipped: 0, rust_ms: 0.0, ruby_ms: 0.0 }) do |agg, sum|
    agg.each { |key, value| sum[key] += value }
  end
  rows = per_type.map do |type, a|
    speedup = a[:rust_ms].positive? ? format("%.1fx", a[:ruby_ms] / a[:rust_ms]) : "n/a"
    "| #{type} | #{a[:timed]} | #{a[:skipped]} | #{fmt(a[:rust_ms])} | #{fmt(a[:ruby_ms])} | #{speedup} |"
  end
  total_speedup = total[:rust_ms].positive? ? format("%.1fx", total[:ruby_ms] / total[:rust_ms]) : "n/a"
  reason_rows = reasons.empty? ? ["| (none) | 0 |"] : reasons.map { |reason, count| "| #{reason} | #{count} |" }

  <<~MD
    # Rust vs Ruby render timings

    Generated by `ruby benchmarks/bench.rb`; do not edit by hand. Regenerate after any
    change to the golden corpus or either renderer.

    ## Environment

    | Item | Value |
    |---|---|
    | Generated | #{Time.now.strftime("%Y-%m-%d %H:%M %Z")} |
    | Host | #{command_output("uname -srm")} |
    | CPU | #{command_output("sysctl -n machdep.cpu.brand_string")} |
    | Ruby | #{RUBY_DESCRIPTION} |
    | rustc | #{command_output("rustc -V")} |
    | cargo | #{command_output("cargo -V")} |
    | Ruby sirena checkout | #{RUBY_ROOT} (#{ruby_checkout_rev}) |
    | Rust library | target/release/libsirena_ffi (release build, FFI call included) |
    | Corpus | golden/*/*.mmd: #{rust_count} cases in the Rust run, #{ruby_count} in the Ruby run |
    | Iterations | #{ITERATIONS} timed per case after one untimed warm-up |

    ## Per-type results

    Times are the sum over timed cases of each case's median render time.
    Speedup = Ruby ms / Rust ms.

    | Type | Cases timed | Cases skipped | Rust ms | Ruby ms | Speedup |
    |---|---:|---:|---:|---:|---:|
    #{rows.join("\n")}
    | **Total** | #{total[:timed]} | #{total[:skipped]} | #{fmt(total[:rust_ms])} | #{fmt(total[:ruby_ms])} | #{total_speedup} |

    ## Skipped cases

    A case is skipped when either side does not render it. Rust `Unported` cases are
    left to the Ruby fallback and are not timed on either side.

    | Reason | Cases |
    |---|---:|
    #{reason_rows.join("\n")}

    ## Notes

    - Each file is one run. Small types move by tens of percent between runs
      (class_diagram read 3.0x in one run and 1.8x in the next), so treat speedups
      as rough orders of magnitude.
    - Only timing is measured; the outputs of the two renderers are not compared.
      The Ruby side is pinned to 2026-01-01 as in scripts/golden_generate.rb;
      `sirena_render` takes no date argument.

    ## Rerun

    From the Rust checkout (needs `cargo` on PATH, and the Ruby checkout present):

    ```sh
    ruby benchmarks/bench.rb
    # optional: SIRENA_RUBY_ROOT=/path/to/sirena-checkout BENCH_ITERATIONS=5 ruby benchmarks/bench.rb
    ```

    The script builds `sirena-ffi` in release mode, times each side in its own
    process, and rewrites this file.
  MD
end

if ARGV.first == "--worker"
  run_worker(ARGV[1], ARGV[2])
else
  build_ffi
  rust = run_side("rust")
  ruby = run_side("ruby")
  per_type, reasons = summarize(rust, ruby)
  File.write(OUT, render_markdown(per_type, reasons, rust["cases"].size, ruby["cases"].size))
  puts "wrote #{OUT}"
end
