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
    # R2 speaks S3; the skips cover the AWS-only calls R2 doesn't implement
    backend.s3 = {
      bucket = curator.stateBucket;
      key = "meldweb-curator/terraform.tfstate";
      region = "auto";
      endpoints.s3 = "https://${accountId}.r2.cloudflarestorage.com";
      use_path_style = true;
      skip_credentials_validation = true;
      skip_region_validation = true;
      skip_requesting_account_id = true;
      skip_metadata_api_check = true;
      skip_s3_checksum = true;
    };
  };

  # token comes from CLOUDFLARE_API_TOKEN
  provider.cloudflare = {};

  data.cloudflare_zone.curator.filter.name = zone;

  # From the GitHub App's settings page; `infra` passes it in from secrets.env
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
}
