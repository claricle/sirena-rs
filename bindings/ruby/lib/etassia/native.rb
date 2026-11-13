# frozen_string_literal: true

require "json"
require "rbconfig"
require "ffi"

module Sirena
  # Renders through the sirena-ffi C ABI (see crates/sirena-ffi/README.md).
  #
  # The shared library is looked up in this order:
  #   1. ENV["SIRENA_NATIVE_LIB"] — exact file path; when set, nothing else is searched.
  #   2. lib/sirena/native/ inside an installed platform gem (precompiled, no compile at install).
  #   3. <repo>/target/release, then <repo>/target/debug.
  # When no library is found, or the Rust core answers `Unported`, render
  # falls back to a pure-Ruby callable (default: the `sirena` gem's Sirena.render).
  module Native
    LIBRARY_ENV = "SIRENA_NATIVE_LIB"
    REPO_ROOT = File.expand_path("../../../..", __dir__)
    PACKAGED_DIR = File.join(__dir__, "native").freeze
    SEARCH_DIRS = [PACKAGED_DIR,
                   File.join(REPO_ROOT, "target", "release"),
                   File.join(REPO_ROOT, "target", "debug")].freeze

    # The Rust core declined this diagram type; the caller should fall back.
    class Unported < StandardError; end

    # Any other error the native library reports (parse, encoding, ...). Never falls back.
    class RenderError < StandardError
      attr_reader :error_class, :line, :col

      def initialize(error_class, message, line, col)
        @error_class = error_class
        @line = line
        @col = col
        super("#{error_class} at #{line}:#{col}: #{message}")
      end
    end

    # Neither the native library nor the pure-Ruby fallback can render.
    class Unavailable < StandardError; end

    # Loads one cdylib and calls its three entry points.
    class Library
      # Matches `struct SirenaBuf` in crates/sirena-ffi/src/lib.rs (24 bytes on 64-bit).
      class Buf < FFI::Struct
        layout :ptr, :pointer,
               :len, :size_t,
               :status, :int
      end

      attr_reader :path

      def initialize(path)
        @path = path
        buf = Buf
        @lib = Module.new do
          extend FFI::Library

          ffi_lib path
          attach_function :sirena_render, %i[pointer size_t pointer], buf.by_value
          attach_function :sirena_detect, %i[pointer size_t], buf.by_value
          attach_function :sirena_free, [buf.by_value], :void
        end
      end

      def render(source)
        ptr, len = input(source)
        take(@lib.sirena_render(ptr, len, nil))
      end

      def detect(source)
        ptr, len = input(source)
        take(@lib.sirena_detect(ptr, len))
      end

      private

      def input(source)
        bytes = source.to_s.encode(Encoding::UTF_8).b
        ptr = FFI::MemoryPointer.new(:uint8, [bytes.bytesize, 1].max)
        ptr.put_bytes(0, bytes)
        [ptr, bytes.bytesize]
      end

      # Copies the buffer out, always frees it, then maps status to result or error.
      def take(buf)
        body = buf[:ptr].read_bytes(buf[:len]).force_encoding(Encoding::UTF_8)
        status = buf[:status]
        @lib.sirena_free(buf)
        return body if status.zero?

        raise_error(body)
      end

      def raise_error(json)
        info = JSON.parse(json)
        raise Unported, info["message"] if info["class"] == "Unported"

        raise RenderError.new(info["class"], info["message"], info["line"], info["col"])
      end
    end

    class << self
      # Pure-Ruby renderer used when the native path cannot answer. Injectable.
      attr_writer :fallback

      def fallback
        @fallback ||= method(:pure_ruby_render)
      end

      # Path of the library that would be loaded, or nil.
      def library_path
        override = ENV.fetch(LIBRARY_ENV, nil)
        return (File.file?(override) ? override : nil) if override && !override.empty?

        SEARCH_DIRS.map { |dir| File.join(dir, library_filename) }.find { |path| File.file?(path) }
      end

      def available?
        !library_path.nil?
      end

      # SVG for `source`. Uses the native library, or the fallback when it is
      # missing or reports Unported. Other native errors raise RenderError.
      def render(source, fallback: self.fallback)
        lib = library
        return fallback.call(source) if lib.nil?

        lib.render(source)
      rescue Unported
        fallback.call(source)
      end

      # Diagram type name for `source`. Native only; raises Unavailable without a library.
      def detect(source)
        lib = library or raise Unavailable,
                               "#{LIBRARY_ENV} not set and no #{library_filename} in the gem or under target/"

        lib.detect(source)
      end

      def library
        path = library_path
        return nil if path.nil?

        @libraries ||= {}
        @libraries[path] ||= Library.new(path)
      end

      private

      def library_filename
        case RbConfig::CONFIG["host_os"]
        when /darwin/ then "libsirena_ffi.dylib"
        when /mswin|mingw/ then "sirena_ffi.dll"
        else "libsirena_ffi.so"
        end
      end

      def pure_ruby_render(source)
        begin
          require "sirena"
        rescue LoadError
          raise Unavailable, "native library not found and the pure-Ruby 'sirena' gem is not installed"
        end
        raise Unavailable, "the 'sirena' gem does not define Sirena.render" unless Sirena.respond_to?(:render)

        Sirena.render(source)
      end
    end
  end
end
