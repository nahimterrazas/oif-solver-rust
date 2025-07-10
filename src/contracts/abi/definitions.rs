use std::collections::HashMap;
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct ContractAbi {
    pub functions: Vec<String>,
    pub events: Vec<String>,
}

pub trait AbiProvider: Send + Sync {
    fn get_contract_abi(&self, contract_name: &str) -> Result<&ContractAbi>;
    fn get_function_signature(&self, contract: &str, function: &str) -> Result<String>;
}

pub struct AbiRegistry {
    abis: HashMap<String, ContractAbi>,
}

impl AbiRegistry {
    pub fn new() -> Self {
        let mut abis = HashMap::new();
        
        // SettlerCompact - CRITICAL: Matching TypeScript exactly
        // Selector: 0xdd1ff485
        abis.insert("SettlerCompact".to_string(), ContractAbi {
            functions: vec![
                // EXACT signature that produces 0xdd1ff485 selector
                "finalise((address,uint256,uint256,uint32,uint32,address,uint256[2][],(bytes32,bytes32,uint256,bytes32,uint256,bytes32,bytes,bytes)[]),bytes,uint32[],bytes32[],bytes32,bytes)".to_string(),
            ],
            events: vec![
                "Finalised(bytes32 indexed orderId, bytes32 indexed solver, bytes32 destination)".to_string(),
            ],
        });
        
        // CoinFiller
        abis.insert("CoinFiller".to_string(), ContractAbi {
            functions: vec![
                "fill(uint32,bytes32,(bytes32,bytes32,uint256,bytes32,uint256,bytes32,bytes,bytes),bytes32)".to_string(),
            ],
            events: vec![
                "OutputFilled(bytes32 indexed orderId, bytes32 solver, uint32 timestamp, (bytes32,bytes32,uint256,bytes32,uint256,bytes32,bytes,bytes))".to_string(),
            ],
        });
        
        // TheCompact
        abis.insert("TheCompact".to_string(), ContractAbi {
            functions: vec![
                "deposit(address,uint256)".to_string(),
                "withdraw(address,uint256)".to_string(),
                "__registerAllocator(address,bytes)".to_string(),
            ],
            events: vec![
                "Deposit(address indexed user, address indexed token, uint256 amount)".to_string(),
                "AllocatorRegistered(uint96 indexed allocatorId, address indexed allocator)".to_string(),
            ],
        });
        
        Self { abis }
    }
}

impl AbiProvider for AbiRegistry {
    fn get_contract_abi(&self, contract_name: &str) -> Result<&ContractAbi> {
        self.abis.get(contract_name)
            .ok_or_else(|| anyhow::anyhow!("Contract ABI not found: {}", contract_name))
    }
    
    fn get_function_signature(&self, contract: &str, function: &str) -> Result<String> {
        let abi = self.get_contract_abi(contract)?;
        abi.functions.iter()
            .find(|f| f.starts_with(function))
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Function not found: {}::{}", contract, function))
    }
} 