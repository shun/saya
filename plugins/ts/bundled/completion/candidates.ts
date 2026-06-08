import type { SayaCompletionCandidate } from "./types.ts";

export interface CandidateRank {
  distance: number;
  kindRank?: number;
}

const candidateRanks: WeakMap<SayaCompletionCandidate, CandidateRank> =
  new WeakMap();

export function setCandidateRank(
  candidate: SayaCompletionCandidate,
  rank: CandidateRank,
): SayaCompletionCandidate {
  candidateRanks.set(candidate, rank);
  return candidate;
}

export function compareCandidates(
  left: SayaCompletionCandidate,
  right: SayaCompletionCandidate,
): number {
  const leftRank = candidateRanks.get(left);
  const rightRank = candidateRanks.get(right);
  if (!leftRank && !rightRank) return left.label.localeCompare(right.label);
  const leftDistance = leftRank?.distance ?? Number.POSITIVE_INFINITY;
  const rightDistance = rightRank?.distance ?? Number.POSITIVE_INFINITY;
  if (leftDistance !== rightDistance) return leftDistance - rightDistance;
  const leftKindRank = leftRank?.kindRank ?? 0;
  const rightKindRank = rightRank?.kindRank ?? 0;
  if (leftKindRank !== rightKindRank) return leftKindRank - rightKindRank;
  if (left.label.length !== right.label.length) {
    return left.label.length - right.label.length;
  }
  return left.label.localeCompare(right.label);
}

export function uniqueByLabel(
  candidates: SayaCompletionCandidate[],
): SayaCompletionCandidate[] {
  const seen: Set<string> = new Set();
  const result: SayaCompletionCandidate[] = [];
  for (const candidate of candidates) {
    const label = String(candidate.label ?? "").trim();
    if (!label || seen.has(label)) continue;
    seen.add(label);
    const normalized = { ...candidate, label };
    const rank = candidateRanks.get(candidate);
    if (rank) candidateRanks.set(normalized, rank);
    result.push(normalized);
  }
  return result;
}
