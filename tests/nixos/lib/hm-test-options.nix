{ lib, ... }:

{
  options = {
    assertions = lib.mkOption {
      type = lib.types.listOf lib.types.anything;
      default = [ ];
    };

    home.activation = lib.mkOption {
      type = lib.types.attrsOf lib.types.anything;
      default = { };
    };

    systemd.user = {
      services = lib.mkOption {
        type = lib.types.attrsOf lib.types.anything;
        default = { };
      };
      sockets = lib.mkOption {
        type = lib.types.attrsOf lib.types.anything;
        default = { };
      };
      tmpfiles.rules = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ ];
      };
    };

    xdg = {
      configHome = lib.mkOption {
        type = lib.types.path;
        default = "/home/test/.config";
      };

      stateHome = lib.mkOption {
        type = lib.types.path;
        default = "/home/test/.local/state";
      };

      configFile = lib.mkOption {
        type = lib.types.attrsOf (
          lib.types.submodule (
            { config, ... }:
            {
              options = {
                source = lib.mkOption { type = lib.types.path; };
                text = lib.mkOption {
                  type = lib.types.str;
                  default = builtins.readFile config.source;
                };
              };
            }
          )
        );
        default = { };
      };
    };
  };
}
