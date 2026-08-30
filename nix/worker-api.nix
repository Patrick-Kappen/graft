let
  range = builtins.fromJSON (builtins.readFile ../crates/graft/worker-api-range.json);
in
{
  inherit range;
  isCompatible =
    workerRange: requiredRange:
    workerRange != null
    && workerRange.major == requiredRange.major
    && workerRange.min_minor <= requiredRange.min_minor
    && workerRange.max_minor >= requiredRange.max_minor;
}
