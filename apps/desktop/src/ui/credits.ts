// Provider balances shared by the usage view and session composer.

export interface ProviderCredits {
  provider: string;
  scope: string;
  unit: string;
  remaining: string | null;
  total: string | null;
  used: string | null;
  resets: string | null;
}


/// Keep decimal precision and distinguish an unknown balance from zero.
export function creditAmount(credit: ProviderCredits, value: string | null) {
  if (value == null) return "unavailable";
  return credit.unit === "usd" ? `$${value}` : value;
}

export function creditScope(scope: string) {
  return ({ account: "Account", api_key: "API key limit", budget: "Budget" })[scope] ?? scope;
}

export function providerName(provider: string) {
  return provider === "openrouter" ? "OpenRouter" : provider;
}

