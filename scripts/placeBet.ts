import * as anchor from "@coral-xyz/anchor";
import { BN, Program } from "@coral-xyz/anchor";
import { PublicKey, SystemProgram } from "@solana/web3.js";
import idl from "../target/idl/polyswarms.json" assert { type: "json" };

const PROGRAM_ID = new PublicKey("REPLACE_WITH_PROGRAM_ID");

// usage: npm run bet -- <MARKET_PUBKEY> <YES|NO> <AMOUNT_SOL>
function pda(seed: string, market: PublicKey) {
  return PublicKey.findProgramAddressSync([Buffer.from(seed), market.toBuffer()], PROGRAM_ID);
}

function posPda(market: PublicKey, owner: PublicKey, sideIndex: number) {
  return PublicKey.findProgramAddressSync(
    [Buffer.from("position"), market.toBuffer(), owner.toBuffer(), Buffer.from([sideIndex])],
    PROGRAM_ID
  );
}

(async () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = new Program(idl as any, PROGRAM_ID, provider) as any;

  const marketPk = new PublicKey(process.argv[2]);
  const sideStr = (process.argv[3] || "YES").toUpperCase();
  const amountSol = parseFloat(process.argv[4] || "0.1");
  const lamports = Math.floor(amountSol * 1_000_000_000);

  const side = sideStr === "YES" ? { yes: {} } : { no: {} };
  const sideIndex = sideStr === "YES" ? 1 : 2;

  const [vaultYes] = pda("vault_yes", marketPk);
  const [vaultNo] = pda("vault_no", marketPk);
  const [position] = posPda(marketPk, provider.wallet.publicKey, sideIndex);

  await program.methods
    .placeBet(side, sideIndex, new BN(lamports))
    .accounts({
      market: marketPk,
      vaultYes,
      vaultNo,
      position,
      user: provider.wallet.publicKey,
      systemProgram: SystemProgram.programId
    })
    .rpc();

  console.log(`Bet placed: ${sideStr} ${amountSol} SOL on ${marketPk.toBase58()}`);
})();
