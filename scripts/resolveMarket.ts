import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { PublicKey } from "@solana/web3.js";
import idl from "../target/idl/polyswarms.json" assert { type: "json" };

const PROGRAM_ID = new PublicKey("REPLACE_WITH_PROGRAM_ID");

// usage: npm run resolve -- <MARKET_PUBKEY> <YES|NO|UNSET>
function parseOutcome(s: string) {
  if (s === "YES") return { yes: {} };
  if (s === "NO") return { no: {} };
  return { unset: {} };
}

(async () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = new Program(idl as any, PROGRAM_ID, provider) as any;

  const marketPk = new PublicKey(process.argv[2]);
  const outcome = parseOutcome(process.argv[3] || "UNSET");

  await program.methods
    .resolveMarket(outcome)
    .accounts({
      market: marketPk,
      resolver: provider.wallet.publicKey
    })
    .rpc();

  console.log("Resolved market:", marketPk.toBase58(), "->", process.argv[3]);
})();
