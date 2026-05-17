type VerificationMarker = {
  verifiedAt: string;
  cwd: string;
  note: string;
};

const markerPath = ".codex/.verification/last-success.json";

await Deno.mkdir(".codex/.verification", { recursive: true });

const marker: VerificationMarker = {
  verifiedAt: new Date().toISOString(),
  cwd: Deno.cwd(),
  note:
    "Updated after relevant verification commands and behavior checks completed successfully.",
};

await Deno.writeTextFile(markerPath, `${JSON.stringify(marker, null, 2)}\n`);
console.log(`Updated ${markerPath}`);
