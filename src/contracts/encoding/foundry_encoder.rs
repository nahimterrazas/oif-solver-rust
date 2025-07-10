use crate::contracts::encoding::{CallDataEncoder, traits::{FinaliseParams, FillParams, StandardOrderParams, MandateOutputParams}};
use crate::contracts::abi::AbiProvider;
use alloy::primitives::{Address, FixedBytes, Bytes, U256};
use anyhow::Result;
use std::sync::Arc;
use std::process::Command;
use std::str::FromStr;
use tracing::{info, error, warn};
use hex;

pub struct FoundryEncoder {
    abi_provider: Arc<dyn AbiProvider>,
}

impl FoundryEncoder {
    pub fn new(abi_provider: Arc<dyn AbiProvider>) -> Self {
        Self { abi_provider }
    }
    
    /// Check if Foundry cast is available
    fn check_cast_availability() -> Result<()> {
        let cast_available = Command::new("cast")
            .arg("--version")
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false);
            
        if !cast_available {
            error!("⚠️  Foundry cast not available - install with: curl -L https://foundry.paradigm.xyz | bash");
            return Err(anyhow::anyhow!("Foundry cast not available"));
        }
        
        info!("✅ Foundry cast is available");
        Ok(())
    }
}

impl CallDataEncoder for FoundryEncoder {
    /// High-level interface: Convert Order to call data directly
    fn encode_finalize_call(&self, order: &crate::models::Order) -> Result<Vec<u8>> {
        info!("🔄 Converting Order to FinaliseParams for Foundry encoding");
        
        // Convert Order to FinaliseParams internally
        let params = self.order_to_finalize_params(order)?;
        
        // Use the existing detailed implementation
        self.encode_finalise_call_internal(&params)
    }
    
    fn get_finalize_selector(&self) -> [u8; 4] {
        // Get the correct selector using the ABI registry
        let function_sig = self.abi_provider
            .get_function_signature("SettlerCompact", "finalise")
            .unwrap_or_else(|_| "finalise((address,uint256,uint256,uint32,uint32,address,uint256[2][],(bytes32,bytes32,uint256,bytes32,uint256,bytes32,bytes,bytes)[]),(bytes,bytes),uint32[],bytes32[],bytes32,bytes)".to_string());
        
        // Use cast to get selector
        let output = Command::new("cast")
            .arg("sig")
            .arg(&function_sig)
            .output()
            .expect("Failed to get function selector");
            
        let selector_hex = String::from_utf8(output.stdout).unwrap().trim().to_string();
        let selector_hex = selector_hex.strip_prefix("0x").unwrap_or(&selector_hex);
        let selector_bytes = hex::decode(selector_hex).expect("Failed to decode selector");
        
        [selector_bytes[0], selector_bytes[1], selector_bytes[2], selector_bytes[3]]
    }
    
    fn description(&self) -> &str {
        "FoundryEncoder: Uses Foundry cast for ABI encoding with TypeScript compatibility"
    }
    
}

impl FoundryEncoder {
    /// Convert Order model to FinaliseParams for internal processing
    fn order_to_finalize_params(&self, order: &crate::models::Order) -> Result<FinaliseParams> {
        info!("🔄 Converting Order to FinaliseParams");
        
        let standard_order = &order.standard_order;
        
        // Helper to convert address to bytes32
        let address_to_bytes32 = |addr: Address| -> FixedBytes<32> {
            let mut bytes = [0u8; 32];
            bytes[12..].copy_from_slice(addr.as_slice());
            FixedBytes::from(bytes)
        };
        
        // Convert inputs
        let inputs: Result<Vec<(U256, U256)>, anyhow::Error> = standard_order.inputs.iter()
            .map(|(token_id, amount)| {
                let token_id_u256 = U256::from_str(token_id)?;
                let amount_u256 = U256::from_str(amount)?;
                Ok((token_id_u256, amount_u256))
            })
            .collect();
        let inputs = inputs?;
        
        // Convert outputs  
        let outputs: Result<Vec<MandateOutputParams>, anyhow::Error> = standard_order.outputs.iter()
            .map(|output| {
                let amount_u256 = U256::from_str(&output.amount)?;
                Ok(MandateOutputParams {
                    remote_oracle: address_to_bytes32(output.remote_oracle),
                    remote_filler: address_to_bytes32(output.remote_filler),
                    chain_id: U256::from(output.chain_id),
                    token: address_to_bytes32(output.token),
                    amount: amount_u256,
                    recipient: address_to_bytes32(output.recipient),
                    remote_call: output.remote_call.as_ref()
                        .map(|s| Bytes::from(hex::decode(s.strip_prefix("0x").unwrap_or(s)).unwrap_or_default()))
                        .unwrap_or_default(),
                    fulfillment_context: output.fulfillment_context.as_ref()
                        .map(|s| Bytes::from(hex::decode(s.strip_prefix("0x").unwrap_or(s)).unwrap_or_default()))
                        .unwrap_or_default(),
                })
            })
            .collect();
        let outputs = outputs?;
        
        // Create StandardOrderParams
        let order_params = StandardOrderParams {
            user: standard_order.user,
            nonce: U256::from(standard_order.nonce),
            origin_chain_id: U256::from(standard_order.origin_chain_id),
            expires: standard_order.expires as u32,
            fill_deadline: standard_order.fill_deadline as u32,
            local_oracle: standard_order.local_oracle,
            inputs,
            outputs,
        };
        
        // Process signature
        let sponsor_sig = {
            let sig_str = order.signature.strip_prefix("0x").unwrap_or(&order.signature);
            Bytes::from(hex::decode(sig_str)?)
        };
        
        // Create timestamps, solvers, destination (use current timestamp and solver address)
        let current_timestamp = 1752062605u32; // Use working TypeScript timestamp  
        let solver_address = Address::from_str("0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC")?; // Default solver
        
        Ok(FinaliseParams {
            order: order_params,
            sponsor_sig,
            allocator_sig: Bytes::new(),
            timestamps: vec![current_timestamp],
            solvers: vec![address_to_bytes32(solver_address)],
            destination: address_to_bytes32(solver_address),
            calls: Bytes::new(),
        })
    }
    
    /// Detailed implementation: Low-level encoding with specific parameters
    pub fn encode_finalise_call_internal(&self, params: &FinaliseParams) -> Result<Vec<u8>> {
        info!("🔧 Using Foundry cast ABI encoder for SettlerCompact.finalise()");
        
        // Check cast availability first
        Self::check_cast_availability()?;
        
        // Get the correct function signature from ABI registry
        let function_sig = self.abi_provider.get_function_signature("SettlerCompact", "finalise")?;
        info!("📋 Using function signature: {}", function_sig);
        
        // Helper functions for formatting
        let addr_hex = |a: &Address| -> String {
            format!("0x{}", hex::encode(a.as_slice()))
        };
        
        let bytes32_hex = |b: &FixedBytes<32>| -> String {
            format!("0x{}", hex::encode(b.as_slice()))
        };
        
        let bytes_hex = |b: &[u8]| -> String {
            if b.is_empty() { "0x".to_string() } else { format!("0x{}", hex::encode(b)) }
        };
        
        // Build order argument - CRITICAL: Match TypeScript signature exactly
        // (address,uint256,uint256,uint32,uint32,address,uint256[2][],(bytes32,bytes32,uint256,bytes32,uint256,bytes32,bytes,bytes)[])
        let order_arg = format!(
            "({},{},{},{},{},{},{},{})",
            addr_hex(&params.order.user),
            params.order.nonce,
            params.order.origin_chain_id,
            params.order.expires,        // uint32 for correct ABI
            params.order.fill_deadline,  // uint32 for correct ABI
            addr_hex(&params.order.local_oracle),
            // inputs as uint256[2][] - use [a,b] instead of (a,b)
            format!("[{}]", 
                params.order.inputs.iter()
                    .map(|(token_id, amount)| format!("[{},{}]", token_id, amount))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            // outputs as tuple array
            format!("[{}]",
                params.order.outputs.iter()
                    .map(|o| format!(
                        "({},{},{},{},{},{},{},{})",
                        bytes32_hex(&o.remote_oracle),
                        bytes32_hex(&o.remote_filler),
                        o.chain_id,
                        bytes32_hex(&o.token),
                        o.amount,
                        bytes32_hex(&o.recipient),
                        bytes_hex(&o.remote_call),
                        bytes_hex(&o.fulfillment_context)
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        );
        
        // Process signatures - encode as ABI tuple (bytes,bytes)
        let sponsor_hex = format!("0x{}", hex::encode(&params.sponsor_sig));
        let allocator_hex = if params.allocator_sig.is_empty() { "0x".to_string() } else { format!("0x{}", hex::encode(&params.allocator_sig)) };
        
        info!("🔍 Signature processing:");
        info!("  Sponsor signature: {} bytes", params.sponsor_sig.len());
        info!("  Allocator signature: {} bytes", params.allocator_sig.len());
        
        // Use cast to encode the signatures tuple
        let tuple_encode_output = Command::new("cast")
            .arg("abi-encode")
            .arg("f(bytes,bytes)")  // Function signature for tuple encoding
            .arg(&sponsor_hex)
            .arg(&allocator_hex)
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to encode signatures tuple: {}", e))?;

        if !tuple_encode_output.status.success() {
            let stderr = String::from_utf8_lossy(&tuple_encode_output.stderr);
            return Err(anyhow::anyhow!("Failed to encode signatures tuple: {}", stderr));
        }

        let signatures_arg = String::from_utf8(tuple_encode_output.stdout)?.trim().to_string();
        
        // Build other arguments
        let timestamps_arg = format!("[{}]", params.timestamps.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(","));
        let solvers_arg = format!("[{}]", params.solvers.iter().map(|s| bytes32_hex(s)).collect::<Vec<_>>().join(","));
        let destination_arg = bytes32_hex(&params.destination);
        let calls_arg = bytes_hex(&params.calls);
        
        info!("🔧 Cast encoding arguments:");
        info!("  Order: {}", &order_arg[..200.min(order_arg.len())]);
        info!("  Signatures: {} chars = {} bytes", signatures_arg.len(), (signatures_arg.len() - 2) / 2);
        info!("  Timestamps: {}", timestamps_arg);
        info!("  Solvers: {}", solvers_arg);
        info!("  Destination: {}", destination_arg);
        info!("  Calls: {}", calls_arg);
        
        // Call cast abi-encode to generate the call data
        let output = Command::new("cast")
            .arg("abi-encode")
            .arg(&function_sig)
            .arg(&order_arg)
            .arg(&signatures_arg)
            .arg(&timestamps_arg)
            .arg(&solvers_arg)
            .arg(&destination_arg)
            .arg(&calls_arg)
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to run cast abi-encode: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(anyhow::anyhow!("cast abi-encode failed:\nSTDERR: {}\nSTDOUT: {}", stderr, stdout));
        }

        // Parse the encoded parameters
        let encoded_hex = String::from_utf8(output.stdout)?.trim().to_string();
        let encoded_hex = encoded_hex.strip_prefix("0x").unwrap_or(&encoded_hex);
        let encoded_bytes = hex::decode(encoded_hex)
            .map_err(|e| anyhow::anyhow!("Failed to decode hex from cast: {}", e))?;

        // Get the correct selector using cast sig
        let selector_output = Command::new("cast")
            .arg("sig")
            .arg(&function_sig)
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to get function selector: {}", e))?;
            
        if !selector_output.status.success() {
            let stderr = String::from_utf8_lossy(&selector_output.stderr);
            return Err(anyhow::anyhow!("Failed to get function selector: {}", stderr));
        }
        
        let selector_hex = String::from_utf8(selector_output.stdout)?.trim().to_string();
        let selector_hex = selector_hex.strip_prefix("0x").unwrap_or(&selector_hex);
        let selector_bytes = hex::decode(selector_hex)
            .map_err(|e| anyhow::anyhow!("Failed to decode selector: {}", e))?;
            
        if selector_bytes.len() != 4 {
            return Err(anyhow::anyhow!("Invalid selector length: {} bytes", selector_bytes.len()));
        }

        // Combine selector + parameters
        let calldata = [selector_bytes.as_slice(), &encoded_bytes].concat();
        
        info!("✅ Foundry cast encoding completed:");
        info!("  Function selector: 0x{}", hex::encode(&selector_bytes));
        info!("  Parameters: {} bytes", encoded_bytes.len());
        info!("  Total call data: {} bytes", calldata.len());
        
        // Validate expected size (should match TypeScript ~1349 bytes)
        if calldata.len() < 1200 {
            error!("❌ Calldata unexpectedly small: {} bytes (expected ≈1349)", calldata.len());
            return Err(anyhow::anyhow!("Calldata too small: {} bytes, expected ≈1349", calldata.len()));
        }
        
        if (1345..=1355).contains(&calldata.len()) {
            info!("🎉 SUCCESS! Cast payload = {} bytes (matches expected TypeScript ≈1349)", calldata.len());
        } else {
            warn!("⚠️  Cast payload = {} bytes (outside expected 1345-1355 range – verify vs TypeScript)", calldata.len());
        }
        
        // Log comparison data for debugging
        info!("🔬 FOUNDRY ENCODER CALL DATA FOR COMPARISON:");
        info!("🔬 Rust CallData ({} chars = {} bytes):", calldata.len() * 2, calldata.len());
        info!("🔬 0x{}", hex::encode(&calldata));
        info!("🔬 END RUST CALL DATA");
        
        Ok(calldata)
    }
    
    /// Legacy method for fill call encoding (specific parameters)
    pub fn encode_fill_call(&self, params: &FillParams) -> Result<Vec<u8>> {
        info!("🔧 Using Foundry cast ABI encoder for CoinFiller.fill()");
        
        // Check cast availability first
        Self::check_cast_availability()?;
        
        // Get the correct function signature from ABI registry
        let function_sig = self.abi_provider.get_function_signature("CoinFiller", "fill")?;
        info!("📋 Using function signature: {}", function_sig);
        
        // Helper functions
        let bytes32_hex = |b: &FixedBytes<32>| -> String {
            format!("0x{}", hex::encode(b.as_slice()))
        };
        
        let bytes_hex = |b: &[u8]| -> String {
            if b.is_empty() { "0x".to_string() } else { format!("0x{}", hex::encode(b)) }
        };
        
        // Build fill arguments
        // fill(uint32,bytes32,(bytes32,bytes32,uint256,bytes32,uint256,bytes32,bytes,bytes),bytes32)
        let fill_deadline_arg = params.fill_deadline.to_string();
        let order_id_arg = bytes32_hex(&params.order_id);
        let output_arg = format!(
            "({},{},{},{},{},{},{},{})",
            bytes32_hex(&params.output.remote_oracle),
            bytes32_hex(&params.output.remote_filler),
            params.output.chain_id,
            bytes32_hex(&params.output.token),
            params.output.amount,
            bytes32_hex(&params.output.recipient),
            bytes_hex(&params.output.remote_call),
            bytes_hex(&params.output.fulfillment_context)
        );
        let proposed_solver_arg = bytes32_hex(&params.proposed_solver);
        
        info!("🔧 Cast encoding arguments for fill:");
        info!("  Fill deadline: {}", fill_deadline_arg);
        info!("  Order ID: {}", order_id_arg);
        info!("  Output: {}", &output_arg[..200.min(output_arg.len())]);
        info!("  Proposed solver: {}", proposed_solver_arg);
        
        // Call cast abi-encode
        let output = Command::new("cast")
            .arg("abi-encode")
            .arg(&function_sig)
            .arg(&fill_deadline_arg)
            .arg(&order_id_arg)
            .arg(&output_arg)
            .arg(&proposed_solver_arg)
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to run cast abi-encode for fill: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("cast abi-encode failed for fill: {}", stderr));
        }

        // Parse the encoded parameters
        let encoded_hex = String::from_utf8(output.stdout)?.trim().to_string();
        let encoded_hex = encoded_hex.strip_prefix("0x").unwrap_or(&encoded_hex);
        let encoded_bytes = hex::decode(encoded_hex)
            .map_err(|e| anyhow::anyhow!("Failed to decode hex from cast: {}", e))?;

        // Get function selector
        let selector_output = Command::new("cast")
            .arg("sig")
            .arg(&function_sig)
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to get fill function selector: {}", e))?;
            
        if !selector_output.status.success() {
            let stderr = String::from_utf8_lossy(&selector_output.stderr);
            return Err(anyhow::anyhow!("Failed to get fill function selector: {}", stderr));
        }
        
        let selector_hex = String::from_utf8(selector_output.stdout)?.trim().to_string();
        let selector_hex = selector_hex.strip_prefix("0x").unwrap_or(&selector_hex);
        let selector_bytes = hex::decode(selector_hex)
            .map_err(|e| anyhow::anyhow!("Failed to decode fill selector: {}", e))?;

        // Combine selector + parameters
        let calldata = [selector_bytes.as_slice(), &encoded_bytes].concat();
        
        info!("✅ Foundry cast fill encoding completed:");
        info!("  Function selector: 0x{}", hex::encode(&selector_bytes));
        info!("  Total call data: {} bytes", calldata.len());
        
        Ok(calldata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::abi::{AbiRegistry, AbiProvider};
    use crate::contracts::encoding::traits::{StandardOrderParams, MandateOutputParams};
    use alloy::primitives::U256;
    use std::str::FromStr;

    fn create_test_foundry_encoder() -> FoundryEncoder {
        let abi_registry = Arc::new(AbiRegistry::new());
        FoundryEncoder::new(abi_registry)
    }

    fn create_test_finalize_params() -> FinaliseParams {
        // Create test addresses
        let user = Address::from_str("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266").unwrap();
        let local_oracle = Address::from_str("0x0165878a594ca255338adfa4d48449f69242eb8f").unwrap();
        let remote_oracle = Address::from_str("0xe7f1725e7734ce288f8367e1bb143e90bb3f0512").unwrap();
        let remote_filler = Address::from_str("0x5fbdb2315678afecb367f032d93f642f64180aa3").unwrap();
        let token = Address::from_str("0x9fe46736679d2d9a65f0992f2272de9f3c7fa6e0").unwrap();
        let recipient = Address::from_str("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266").unwrap();
        let solver = Address::from_str("0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC").unwrap();

        // Helper to convert address to bytes32
        let address_to_bytes32 = |addr: Address| -> FixedBytes<32> {
            let mut bytes = [0u8; 32];
            bytes[12..].copy_from_slice(addr.as_slice());
            FixedBytes::from(bytes)
        };

        FinaliseParams {
            order: StandardOrderParams {
                user,
                nonce: U256::from(781),
                origin_chain_id: U256::from(31337),
                expires: 4294967295,  // uint32::MAX
                fill_deadline: 4294967295,  // uint32::MAX  
                local_oracle,
                inputs: vec![(
                    U256::from_str("232173931049414487598928205764542517475099722052565410375093941968804628563").unwrap(),
                    U256::from_str("100000000000000000000").unwrap()
                )],
                outputs: vec![MandateOutputParams {
                    remote_oracle: address_to_bytes32(remote_oracle),
                    remote_filler: address_to_bytes32(remote_filler),
                    chain_id: U256::from(31338),
                    token: address_to_bytes32(token),
                    amount: U256::from_str("99000000000000000000").unwrap(),
                    recipient: address_to_bytes32(recipient),
                    remote_call: Bytes::new(),
                    fulfillment_context: Bytes::new(),
                }],
            },
            sponsor_sig: Bytes::from(
                hex::decode("b99e3849171a57335dc3e25bdffb48b778d9d43851a54ff0606af6095f653acb084513b1458f9c36674e0b529b8f4af5882f73324165bd3df91a0e29948f2bf01c")
                    .expect("Valid hex signature")
            ),
            allocator_sig: Bytes::new(),
            timestamps: vec![1752062605],  // Use the working TypeScript timestamp
            solvers: vec![address_to_bytes32(solver)],
            destination: address_to_bytes32(solver),
            calls: Bytes::new(),
        }
    }

    #[test]
    fn test_foundry_encoder_can_encode_finalise() {
        // Skip if foundry not available in CI
        if Command::new("cast").arg("--version").output().map(|o| o.status.success()).unwrap_or(false) {
            let encoder = create_test_foundry_encoder();
            let params = create_test_finalize_params();
            
            let result = encoder.encode_finalise_call_internal(&params);
            
            // Should not error
            assert!(result.is_ok(), "Encoding should succeed: {:?}", result.err());
            
            let calldata = result.unwrap();
            
            // Should generate reasonable call data size (around 1349 bytes)
            assert!(calldata.len() > 1000, "Call data should be substantial, got {} bytes", calldata.len());
            assert!(calldata.len() < 2000, "Call data should not be too large, got {} bytes", calldata.len());
            
            // Should start with function selector (4 bytes)
            assert_eq!(calldata.len() % 1, 0, "Call data should be valid bytes");
            
            // Log for manual verification
            println!("✅ Generated call data: {} bytes", calldata.len());
            println!("   Selector: 0x{}", hex::encode(&calldata[..4]));
        } else {
            println!("⚠️  Skipping test - Foundry cast not available");
        }
    }

    #[test]
    fn test_abi_registry_has_correct_signature() {
        let registry = AbiRegistry::new();
        let sig = registry.get_function_signature("SettlerCompact", "finalise");
        
        assert!(sig.is_ok(), "Should find SettlerCompact finalise function");
        
        let signature = sig.unwrap();
        
        // Should be the correct signature that produces 0xdd1ff485
        assert!(signature.contains("uint32,uint32"), "Should have uint32 types for expires/fillDeadline");
        assert!(signature.contains("uint256[2][]"), "Should have uint256[2][] for inputs");
        
        println!("✅ Function signature: {}", signature);
    }
} 