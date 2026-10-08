# terranix module: Meldweb Curator's Cloudflare side. `deploy` ships the
# Nix-built worker and site; tofu owns the domain it answers on.
{
  lib,
  curator,
  deploy,
  ...
}: let
  inherit (curator) accountId workerName zone hostname;
in {
  terraform = {
    required_providers.cloudflare.source = "cloudflare/cloudflare";
    # The same Postgres on luna as ~/nixconf's infra, reached through
    # PG_CONN_STR (`infra` builds it). Its own schema, because the backend keys
    # state by schema and workspace, and nixconf's already has the default one
    backend.pg.schema_name = "meldweb_curator";
  };

  # token comes from CLOUDFLARE_API_TOKEN
  provider.cloudflare = {};

  data.cloudflare_zone.curator.filter.name = zone;

  # From the GitHub App's settings page; `infra` reads it from 1Password
  variable.github_client_secret = {
    type = "string";
    sensitive = true;
  };

  # terraform_data only runs its provisioner on create, so this re-runs
  # whenever the Nix build changes or the secret is rotated. Only the hash
  # reaches the state
  resource.terraform_data.worker = {
    triggers_replace = [
      (toString deploy)
      "\${sha256(var.github_client_secret)}"
    ];
    provisioner.local-exec = {
      command = lib.getExe deploy;
      environment = {
        CLOUDFLARE_ACCOUNT_ID = accountId;
        GITHUB_CLIENT_SECRET = "\${var.github_client_secret}";
      };
    };
  };

  # Creates the proxied record and the cert. The GitHub App's callback URL
  # has to be https://${hostname}/api/auth/callback
  resource.cloudflare_workers_custom_domain.curator = {
    account_id = accountId;
    zone_id = "\${data.cloudflare_zone.curator.zone_id}";
    inherit hostname;
    service = workerName;
    depends_on = ["terraform_data.worker"];
  };

  # The worker builds redirect_uri from the request's origin, so a page opened
  # over http sends GitHub a callback it doesn't know. Scoped to this host
  # rather than the zone's Always Use HTTPS, which would reach every cramt.dk
  # subdomain. 308 so a POST stays a POST
  resource.cloudflare_ruleset.curator_https = {
    zone_id = "\${data.cloudflare_zone.curator.zone_id}";
    name = "meldweb-curator https";
    kind = "zone";
    phase = "http_request_dynamic_redirect";
    rules = [
      {
        description = "http://${hostname} to https";
        expression = ''(http.host eq "${hostname}" and not ssl)'';
        action = "redirect";
        action_parameters.from_value = {
          status_code = 308;
          target_url.expression = ''concat("https://", http.host, http.request.uri)'';
          preserve_query_string = false;
        };
      }
    ];
  };
}
