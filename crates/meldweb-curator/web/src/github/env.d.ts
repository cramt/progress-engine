interface ImportMetaEnv {
  /** The GitHub App's slug, for its install URL. */
  readonly VITE_GITHUB_APP_SLUG?: string;
  /** `1` fakes GitHub and the worker in the page; `no-repo` and `no-install` start before onboarding. */
  readonly VITE_MOCK_GITHUB?: string;
}
