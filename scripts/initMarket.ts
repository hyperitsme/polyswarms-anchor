import * as anchor from "@coral-xyz/anchor";
import { BN, Program } from "@coral-xyz/anchor";
import { Keypair, PublicKey, SystemProgram } from "@solana/web3.js";
import idl from "../target/idl/polyswarms.json" assert { type: "json" };

const PROGRAM_ID = new PublicKey("REPLACE_WITH_PROGRAM_ID");

function pda(seed: string, market: PublicKey) {
  return PublicKey.findProgramAddressSync([Buffer.from(seed), market.toBuffer()], PROGRAM_ID);
}

(async () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = new Program(idl as any, PROGRAM_ID, provider) as any;

  const market = Keypair.generate();

  const question = "Will BTC close above $70k this month?";
  const closeTime = Math.floor(Date.now() / 1000) + 7 * 24 * 3600; // +7 days
  const feeBps = 200; // 2%
  const resolver = provider.wallet.publicKey; // replace with your multisig if needed

  const [vaultYes] = pda("vault_yes", market.publicKey);
  const [vaultNo] = pda("vault_no", market.publicKey);
  const [feeVault] = pda("fee_vault", market.publicKey);

  await program.methods
    .initializeMarket(question, new BN(closeTime), feeBps, resolver)
    .accounts({
      market: market.publicKey,
      vaultYes,
      vaultNo,
      feeVault,
      authority: provider.wallet.publicKey,
      systemProgram: SystemProgram.programId
    })
    .signers([market])
    .rpc();

  console.log("Market created:", market.publicKey.toBase58());
  console.log("Vault YES:", vaultYes.toBase58());
  console.log("Vault NO :", vaultNo.toBase58());
  console.log("Fee Vault:", feeVault.toBase58());
})();
