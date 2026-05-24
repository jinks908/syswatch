{
  lib,
  rustPlatform,
  pkg-config,
  libpcap,
}:

rustPlatform.buildRustPackage {
  pname = "syswatch";
  version = "0.6.0";

  src = lib.cleanSource ./.;

  cargoLock.lockFile = ./Cargo.lock;

  nativeBuildInputs = [ pkg-config ];

  # libpcap pulled in transitively by `netwatch-sdk` (used by the Net tab
  # for per-interface counters via the SDK's packet helpers). macpow on
  # macOS uses IOKit + SMC from the base SDK — no extra Nix inputs.
  # The `gpu-nvidia` Cargo feature pulls `nvml-wrapper` which needs
  # `nvml` at link/runtime; it's opt-in (default = []) so the default
  # `nix build` doesn't include it.
  buildInputs = [ libpcap ];

  meta = {
    description = "Single-host, read-only system diagnostics TUI. Twelve tabs covering CPU, memory, disks, processes, GPU, power, services, network, plus a Timeline scrubber and an Insights anomaly engine. Sibling to netwatch.";
    homepage = "https://github.com/matthart1983/syswatch";
    license = lib.licenses.mit;
    mainProgram = "syswatch";
  };
}
