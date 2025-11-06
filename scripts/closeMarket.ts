import * as anchor from "@coral-xyz/anchor";
import { Program, web3 } from "@coral-xyz/anchor";
import { PublicKey } from "@solana/web3.js";
import idl from "../target/idl/polyswarms.json" assert { type: "json" };

const PROGRAM_ID = new PublicKey("REPLACE_WITH_PROGRAM_ID");

// usage: npm run close -- <MARKET_PUBKEY>
(async () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = new Program(idl as any, PROGRAM_ID, provider) as any;

  const marketPk = new PublicKey(process.argv[2]);
  await program.methods
    .closeMarket()
    .accounts({ market: marketPk })
    .rpc();

  console.log("Closed market:", marketPk.toBase58());
})();
