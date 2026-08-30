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

    xdg.configFile = lib.mkOption {
      type = lib.types.attrsOf (
        lib.types.submodule {
          options.source = lib.mkOption { type = lib.types.path; };
        }
      );
      default = { };
    };
  };
}
