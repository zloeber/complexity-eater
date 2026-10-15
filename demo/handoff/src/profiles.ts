export const PROFILE_IDS = ["avery", "jordan", "riley"] as const;

export type ProfileId = (typeof PROFILE_IDS)[number];

export type Profile = {
  id: ProfileId;
  name: string;
  role: string;
  style: string;
  priorities: string[];
};

export const PROFILES: Record<ProfileId, Profile> = {
  avery: {
    id: "avery",
    name: "Avery Chen",
    role: "Engineering manager",
    style:
      "Decisions and dates first. Short sentences. Skip implementation detail that does not move a date.",
    priorities: ["decision needed", "date", "blocking question"],
  },
  jordan: {
    id: "jordan",
    name: "Jordan Hale",
    role: "Senior engineer",
    style:
      "Concrete tasks, acceptance criteria, and dependencies. Question vague alignment language before estimating.",
    priorities: ["action items", "acceptance criteria", "dependencies", "ambiguities"],
  },
  riley: {
    id: "riley",
    name: "Riley Okonkwo",
    role: "Product partner",
    style: "User impact and scope boundaries. Flag work that expands the promise.",
    priorities: ["user impact", "scope", "assumptions"],
  },
};

export function isProfileId(value: unknown): value is ProfileId {
  return typeof value === "string" && PROFILE_IDS.includes(value as ProfileId);
}

export function profile(id: ProfileId): Profile {
  return PROFILES[id];
}
