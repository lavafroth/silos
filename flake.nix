{
  description = "devshell";

  outputs =
    { nixpkgs, ... }:
    let
      forAllSystems =
        f: nixpkgs.lib.genAttrs nixpkgs.lib.systems.flakeExposed (s: f nixpkgs.legacyPackages.${s});
    in
    {
      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell rec {
          packages = with pkgs; [
            stdenv.cc.cc
            pkg-config
            bacon
          ];

          libraries = with pkgs; [
            stdenv.cc.cc.lib
            openssl.dev
          ];

          LD_LIBRARY_PATH = "${pkgs.lib.makeLibraryPath libraries}";
        };
      });
    };
}
