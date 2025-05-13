import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { CollectionPrices } from "../target/types/collection_prices";
import { PublicKey, SystemProgram, Keypair, LAMPORTS_PER_SOL, Connection, clusterApiUrl } from "@solana/web3.js";
import BN from "bn.js";
import fs from "fs";
import {
  getAssociatedTokenAddress,
  TOKEN_PROGRAM_ID,
} from "@solana/spl-token";

describe("collection_prices", () => {
  // Configure the client to use the local cluster.
  anchor.setProvider(anchor.AnchorProvider.env());

  const secretKeyString = fs.readFileSync("/mnt/d/Github/anchorWorld/wallets/devent-test-wallets/devenet-wallet-1.json", { encoding: "utf8" });
  const secretKey = Uint8Array.from(JSON.parse(secretKeyString));
  const keypair = Keypair.fromSecretKey(secretKey);

  const purchaserKeyString = fs.readFileSync("/mnt/d/Github/anchorWorld/wallets/devent-test-wallets/devenet-wallet-2.json", { encoding: "utf8" });
  const purchaserSecretKey = Uint8Array.from(JSON.parse(purchaserKeyString));
  const purchaserKeypair = Keypair.fromSecretKey(purchaserSecretKey);

  const lamportsPaymentMint = new PublicKey(new Uint8Array(32)); // Pubkey.default() in Rust
  const usdcMintDev = new PublicKey("Ejmc1UB4EsES5UfZuoDHnC9B1aVnwrKuQDfKYi6tXjpb"); // USDC on devnet
  const usdcMintMain = new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"); // USDC on mainnet

  console.log(keypair.publicKey)
  
  const collectionAddress = anchor.web3.Keypair.generate();

  const program = anchor.workspace.CollectionPrices as Program<CollectionPrices>;

  const testCollectionAddress = new PublicKey("5ZgHr3wXt5T3CV8WCuwsZsnfRViLLYdKZ1dHDe23mRrV"); 

  let collectionPricesPDA;

  let testCollectionPricesPDA;

  before(async () => {
    const [testCollectionPricesPDA] = await PublicKey.findProgramAddressSync(
      [Buffer.from("prices"), testCollectionAddress.toBuffer()],
      program.programId
    );

    console.log("Derived PDA:", testCollectionPricesPDA.toBase58());

    const account = await program.account.collectionPricesData.fetch(testCollectionPricesPDA);

    console.log("Fetched CollectionPricesData:");
    console.log("- bump:", account.bump);
    console.log("- owner:", account.owner.toBase58());
    console.log("- size:", account.size);
    console.log("- payment_mint:", account.paymentMint.toBase58());
    console.log("- prices:", account.prices.map((bn: anchor.BN) => bn.toString()));

    console.log ("========================");


    [collectionPricesPDA] = await PublicKey.findProgramAddressSync(
      [Buffer.from("prices"), collectionAddress.publicKey.toBuffer()],
      program.programId
    );
  })

  if (true){
    it("Allows user to purchase selected traits", async () => {
      const traitsToBuy = [1]; // indexes (u16) for desired traits
      // const purchaserKeypair = keypair; // reuse the same keypair, or load another if needed
      const collectionPubkey = testCollectionAddress;
    
      // Derive collectionPrices PDA
      const [fetchCollectionPricesPDA] = PublicKey.findProgramAddressSync(
        [Buffer.from("prices"), collectionPubkey.toBuffer()],
        program.programId
      );

      const [userPurchasesPDA] = PublicKey.findProgramAddressSync(
        [
          Buffer.from("purchases"),
          collectionPubkey.toBuffer(),
          purchaserKeypair.publicKey.toBuffer()
        ],
        program.programId 
      );

      const collectionPrices = await program.account.collectionPricesData.fetch(fetchCollectionPricesPDA);
      const owner = collectionPrices.owner as PublicKey;
      const paymentMint = collectionPrices.paymentMint as PublicKey;
    
      
    

      
      const purchasedTraits = [];
      try {
        const userPurchasesData = await program.account.userPurchases.fetch(userPurchasesPDA);
        console.log("!!- user purchases:", userPurchasesData.data)

        const bitmask = userPurchasesData.data as number[];
        

        for (let i = 0; i < collectionPrices.size; i++) {
          const byteIndex = Math.floor(i / 8);
          const bitIndex = i % 8;
          const bitSet = (bitmask[byteIndex] >> bitIndex) & 1;
          purchasedTraits.push(bitSet === 1);
        }
        
        
        console.log("User Purchases Bitmask:", userPurchasesData.data);
      } catch (e) {
        console.log("PDA not initialized:", userPurchasesPDA.toBase58());
        for (let i = 0; i < collectionPrices.size; i++) {
          purchasedTraits.push(false);
        }
      }
      console.log("Purchased Traits Bitmask:", purchasedTraits);

      
        // Prepare accounts

              // Resolve token accounts if needed
      const useTokens = !paymentMint.equals(PublicKey.default);
      if (useTokens) {
        console.log("purchase with tokens");


        const txSim = await program.methods
          .getRoyaltyPubkey()
          .simulate();

        let royaltyPubkey = PublicKey.default;
        const logs = txSim.raw.slice(-10); // recent logs
        for (const log of logs) {
          const match = log.match(/ROYALTY_PUBKEY: ([A-Za-z0-9]+)/);
          if (match) {
            royaltyPubkey = new PublicKey(match[1]);
            console.log("Fetched Royalty Pubkey:", royaltyPubkey.toBase58());
          }
        }

        let purchaserTokenAccount = await getAssociatedTokenAddress(paymentMint, purchaserKeypair.publicKey);
        let ownerTokenAccount = await getAssociatedTokenAddress(paymentMint, owner);
        let royaltyTokenAccount = await getAssociatedTokenAddress(paymentMint, royaltyPubkey);
      
        const accounts: any = {
          purchaser: purchaserKeypair.publicKey,
          collectionAddress: collectionPubkey,
          collectionPricesData: fetchCollectionPricesPDA,
          userPurchases: userPurchasesPDA,
          owner: collectionPrices.owner,
          purchaserTokenAccount: purchaserTokenAccount,
          ownerTokenAccount: ownerTokenAccount,
          royaltyTokenAccount: royaltyTokenAccount,
          commissionTokenAccount: ownerTokenAccount, // change in case there is a comission
          tokenProgram:TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        };
      
        // Call the purchase method
        const tx = await program.methods
          .tokenPurchase(traitsToBuy,PublicKey.default,0)
          .accounts(accounts)
          .signers([purchaserKeypair])
          .rpc();
      
        console.log("Purchase tx signature:", tx);

      }
      else{
        console.log("purchase with lamports")

        const accounts: any = {
          purchaser: purchaserKeypair.publicKey,
          collectionAddress: collectionPubkey,
          collectionPricesData: fetchCollectionPricesPDA,
          userPurchases: userPurchasesPDA,
          owner: collectionPrices.owner,
          systemProgram: SystemProgram.programId,
        };
     

      
        // Call the purchase method
        const tx = await program.methods
          .lamportsPurchase(traitsToBuy,PublicKey.default,0)
          .accounts(accounts)
          .signers([purchaserKeypair])
          .rpc();
      
        console.log("Purchase tx signature:", tx);
      }
    
      // Fetch and verify user's purchase bitmask
      const userPurchases = await program.account.userPurchases.fetch(userPurchasesPDA);
      console.log("User Purchases Bitmask:", userPurchases.data);
    });
  }

  return;
  // 1 solana = 1_000_000_000 1 billion lamports
  it("Is initialized!", async () => {
    const prices = [new BN(100_000), new BN(200_000), new BN(150_000)];
    // Add your test here.
    const tx = await program.methods
    .initializeCollection(prices, lamportsPaymentMint)
    .accounts({
      owner:keypair.publicKey,
      collectionAddress:collectionAddress.publicKey,
      collectionPricesData:collectionPricesPDA,
      systemProgram: SystemProgram.programId,
    })
    .signers([keypair])
    .rpc();


    console.log("Transaction Signature:", tx);
    console.log("CollectionPrices PDA:", collectionPricesPDA.toBase58());

    // ✅ Fetch and log PDA data
    const account = await program.account.collectionPricesData.fetch(collectionPricesPDA);

    console.log("ADDRESS!!");
    console.log(collectionAddress.publicKey);

    console.log("CollectionPricesData:");
    console.log("- bump:", account.bump);
    console.log("- owner:", account.owner.toBase58());
    console.log("- size:", account.size);
    console.log("- payment_mint:", account.paymentMint.toBase58());
    console.log("- prices:", account.prices.map((bn: BN) => bn.toString()));
  });

  return;
  it("Updates prices of existing collection", async () => {
    const updatedPrices = [new BN(1100000), new BN(2100000), new BN(1600000)];
  
    // Recompute PDA
    const [collectionPricesPDA] = await PublicKey.findProgramAddressSync(
      [Buffer.from("prices"), testCollectionAddress.toBuffer()],
      program.programId
    );
  
    const tx = await program.methods
      .updatePrices(updatedPrices)
      .accounts({
        owner: keypair.publicKey,
        collectionAddress: testCollectionAddress,
        collectionPricesData: collectionPricesPDA,
      })
      .signers([keypair])
      .rpc();
  
    console.log("Update transaction signature:", tx);
  });
});

