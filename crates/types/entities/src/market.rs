#![allow(clippy::type_complexity)]
use alloy_primitives::map::HashMap;
use eyre::{eyre, OptionExt, Result};
use std::collections::BTreeMap;
use std::fmt::Display;
use std::sync::Arc;
use tracing::debug;

use crate::{build_swap_path_vec, PoolId};
use crate::{PoolClass, PoolWrapper, Token};
use crate::{SwapPath, SwapPaths};
use loom_types_blockchain::{LoomDataTypes, LoomDataTypesEthereum};

/// The market struct contains all the pools and tokens.
/// It keeps track if a pool is disabled or not and the swap paths.
#[derive(Default, Clone)]
pub struct Market<LDT: LoomDataTypes = LoomDataTypesEthereum> {
    // pool_address -> pool
    pools: HashMap<PoolId<LDT>, PoolWrapper<LDT>>,
    // pool_address -> is_disabled
    pools_disabled: HashMap<PoolId<LDT>, bool>,
    // token_address -> token
    tokens: HashMap<LDT::Address, Arc<Token<LDT>>>,
    // token_from -> token_to
    token_tokens: HashMap<LDT::Address, Vec<LDT::Address>>,
    // token_from -> token_to -> pool_addresses
    token_token_pools: HashMap<LDT::Address, HashMap<LDT::Address, Vec<PoolId<LDT>>>>,
    // token -> pool
    token_pools: HashMap<LDT::Address, Vec<PoolId<LDT>>>,
    // swap_paths
    swap_paths: SwapPaths<LDT>,
}

impl<LDT: LoomDataTypes> Display for Market<LDT> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let token_token_len = self.token_tokens.values().map(|inner| inner.len()).sum::<usize>();
        let token_token_pools_len = self.token_token_pools.values().map(|inner_map| inner_map.len()).sum::<usize>();
        let token_pool_len = self.token_pools.values().map(|inner| inner.len()).sum::<usize>();
        let token_pool_len_max = self.token_pools.values().map(|inner| inner.len()).max().unwrap_or_default();
        let swap_path_len = self.swap_paths.len();
        let swap_path_len_max = self.swap_paths.len_max();

        write!(
            f,
            "Pools: {} Disabled : {} Tokens : {} TT : {} TTP {} TP {}/{} SwapPaths: {}/{}",
            self.pools.len(),
            self.pools_disabled.len(),
            self.tokens.len(),
            token_token_len,
            token_token_pools_len,
            token_pool_len,
            token_pool_len_max,
            swap_path_len,
            swap_path_len_max
        )
    }
}

impl<LDT: LoomDataTypes> Market<LDT> {
    #[inline]
    pub fn is_weth(&self, &address: &LDT::Address) -> bool {
        address.eq(&LDT::WETH)
    }
    /// Add a [`Token`] reference to the market.
    pub fn add_token<T: Into<Arc<Token<LDT>>>>(&mut self, token: T) -> Result<()> {
        let arc_token: Arc<Token<LDT>> = token.into();
        self.tokens.insert(arc_token.get_address(), arc_token);
        Ok(())
    }

    /// Check if the token is a basic token.
    #[inline]
    pub fn is_basic_token(&self, address: &LDT::Address) -> bool {
        self.tokens.get(address).is_some_and(|t| t.is_basic())
    }

    /// Add a new pool to the market if it does not exist or the class is unknown.
    pub fn add_pool<T: Into<PoolWrapper<LDT>>>(&mut self, pool: T) -> Result<()> {
        let pool_contract = pool.into();
        let pool_address = pool_contract.get_pool_id();

        if let Some(pool) = self.pools.get(&pool_address) {
            return Err(eyre!("Pool already exists {:?}", pool.get_address()));
        }

        debug!("Adding pool {:?}", pool_address);

        for (token_from_address, token_to_address) in pool_contract.get_swap_directions().into_iter() {
            self.token_token_pools.entry(token_from_address).or_default().entry(token_to_address).or_default().push(pool_address);
            self.token_tokens.entry(token_from_address).or_default().push(token_to_address);
            // Swap directions are bidirectional, for that reason we only need to add the token_from_address
            self.token_pools.entry(token_from_address).or_default().push(pool_address);
        }

        self.pools.insert(pool_address, pool_contract);

        Ok(())
    }

    /// Add a swap path to the market.
    pub fn add_paths(&mut self, paths: Vec<SwapPath<LDT>>) {
        for path in paths.into_iter() {
            self.swap_paths.add(path);
        }
    }

    /// Get all swap paths from the market by the pool address.
    #[inline]
    pub fn get_pool_paths(&self, pool_address: &PoolId<LDT>) -> Option<Vec<SwapPath<LDT>>> {
        self.swap_paths.get_pool_paths_vec(pool_address)
    }

    /// Get all swap paths from the market by the pool address.
    #[inline]
    pub fn swap_paths(&self) -> Vec<SwapPath<LDT>> {
        self.swap_paths.paths.clone().into_iter().collect::<Vec<_>>()
    }

    /// Get a pool reference by the pool address. If the pool exists but the class is unknown it returns None.
    #[inline]
    pub fn get_pool(&self, address: &PoolId<LDT>) -> Option<&PoolWrapper<LDT>> {
        self.pools.get(address).filter(|&pool_wrapper| pool_wrapper.get_class() != PoolClass::Unknown)
    }

    /// Check if the pool exists in the market.
    #[inline]
    pub fn is_pool(&self, address: &PoolId<LDT>) -> bool {
        self.pools.contains_key(address)
    }

    /// Get a reference to the pools map in the market.
    #[inline]
    pub fn pools(&self) -> &HashMap<PoolId<LDT>, PoolWrapper<LDT>> {
        &self.pools
    }

    /// Set the pool status to ok or not ok.
    pub fn set_pool_disabled(&mut self, address: PoolId<LDT>, disabled: bool) {
        *self.pools_disabled.entry(address).or_insert(false) = disabled;

        self.swap_paths.disable_pool(&address, disabled);
    }

    /// Check if the pool is ok.
    #[inline]
    pub fn is_pool_disabled(&self, address: &PoolId<LDT>) -> bool {
        self.pools_disabled.get(address).is_some_and(|&is_disabled| is_disabled)
    }

    /// Get a [`Token`] reference from the market by the address of the token or create a new one.
    #[inline]
    pub fn get_token_or_default(&self, address: &LDT::Address) -> Arc<Token<LDT>> {
        self.tokens.get(address).map_or(Arc::new(Token::new(*address)), |t| t.clone())
    }

    /// Get a [`Token`] reference from the market by the address of the token.
    #[inline]
    pub fn get_token(&self, address: &LDT::Address) -> Option<Arc<Token<LDT>>> {
        self.tokens.get(address).cloned()
    }

    /// Get all pool addresses that allow to swap from `token_from_address` to `token_to_address`.
    #[inline]
    pub fn get_token_token_pools(&self, token_from_address: &LDT::Address, token_to_address: &LDT::Address) -> Option<Vec<PoolId<LDT>>> {
        self.token_token_pools.get(token_from_address)?.get(token_to_address).cloned()
    }

    /// Get all pool addresses as reference that allow to swap from `token_from_address` to `token_to_address`.
    #[inline]
    pub fn get_token_token_pools_ptr(
        &self,
        token_from_address: &LDT::Address,
        token_to_address: &LDT::Address,
    ) -> Option<&Vec<PoolId<LDT>>> {
        self.token_token_pools.get(token_from_address)?.get(token_to_address)
    }

    /// Get all token addresses that allow to swap from `token_from_address`.
    #[inline]
    pub fn get_token_tokens(&self, token_from_address: &LDT::Address) -> Option<Vec<LDT::Address>> {
        self.token_tokens.get(token_from_address).cloned()
    }

    /// Get all token addresses as reference that allow to swap from `token_from_address`.
    #[inline]
    pub fn get_token_tokens_ptr(&self, token_from_address: &LDT::Address) -> Option<&Vec<LDT::Address>> {
        self.token_tokens.get(token_from_address)
    }

    /// Get all pool addresses that allow to swap `token_address`.
    pub fn get_token_pools(&self, token_from_address: &LDT::Address) -> Option<Vec<PoolId<LDT>>> {
        self.token_pools.get(token_from_address).cloned()
    }

    /// Get all pool addresses as reference that allow to swap `token_address`.
    pub fn get_token_pools_ptr(&self, token_address: &LDT::Address) -> Option<&Vec<PoolId<LDT>>> {
        self.token_pools.get(token_address)
    }

    /// Get all pool addresses as reference that allow to swap `token_address`.
    pub fn get_token_pools_len(&self, token_address: &LDT::Address) -> usize {
        self.token_pools.get(token_address).map_or(0, |t| t.len())
    }
    /// Build a list of swap paths from the given directions.
    pub fn build_swap_path_vec(
        &self,
        directions: &BTreeMap<PoolWrapper<LDT>, Vec<(LDT::Address, LDT::Address)>>,
    ) -> Result<Vec<SwapPath<LDT>>> {
        build_swap_path_vec(self, directions)
    }

    /// get a [`SwapPath`] from the given token and pool addresses.
    pub fn swap_path(&self, token_address_vec: Vec<LDT::Address>, pool_address_vec: Vec<PoolId<LDT>>) -> Result<SwapPath<LDT>> {
        let mut tokens: Vec<Arc<Token<LDT>>> = Vec::new();
        let mut pools: Vec<PoolWrapper<LDT>> = Vec::new();

        for token_address in token_address_vec.iter() {
            tokens.push(self.get_token(token_address).ok_or_eyre("TOKEN_NOT_FOUND")?);
        }
        for pool_address in pool_address_vec.iter() {
            pools.push(self.get_pool(pool_address).cloned().ok_or_eyre("TOKEN_NOT_FOUND")?);
        }

        Ok(SwapPath { tokens, pools, ..Default::default() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock_pool::MockPool;
    use alloy_primitives::Address;
    use eyre::Result;
    use loom_defi_address_book::TokenAddressEth;

    #[test]
    fn test_add_pool() {
        let mut market = Market::default();
        let pool_address = Address::random();
        let token0 = Address::random();
        let token1 = Address::random();
        let mock_pool = MockPool { address: pool_address, token0, token1 };

        let result = market.add_pool(mock_pool);

        assert!(result.is_ok());

        assert_eq!(market.get_pool(&PoolId::Address(pool_address)).unwrap().pool.get_address(), pool_address);

        assert_eq!(*market.get_token_token_pools(&token0, &token1).unwrap().get(0).unwrap(), PoolId::Address(pool_address));
        assert_eq!(*market.get_token_token_pools(&token1, &token0).unwrap().get(0).unwrap(), PoolId::Address(pool_address));

        assert!(market.get_token_tokens(&token0).unwrap().contains(&token1));
        assert!(market.get_token_tokens(&token1).unwrap().contains(&token0));

        assert!(market.get_token_pools(&token0).unwrap().contains(&PoolId::Address(pool_address)));
        assert!(market.get_token_pools(&token1).unwrap().contains(&PoolId::Address(pool_address)));
    }

    #[test]
    fn test_add_token() {
        let mut market = Market::<LoomDataTypesEthereum>::default();
        let token_address = Address::random();

        let result = market.add_token(Arc::new(Token::new(token_address)));

        assert!(result.is_ok());
        assert_eq!(market.get_token(&token_address).unwrap().get_address(), token_address);
    }

    #[test]
    fn test_get_token_default() {
        let market = Market::<LoomDataTypesEthereum>::default();
        let token_address = Address::random();

        let token = market.get_token_or_default(&token_address);

        assert_eq!(token.get_address(), token_address);
    }

    #[test]
    fn test_get_pool() {
        let mut market = Market::default();
        let pool_address = Address::random();
        let mock_pool = MockPool { address: pool_address, token0: Address::ZERO, token1: Address::ZERO };
        market.add_pool(mock_pool.clone());

        let pool = market.get_pool(&PoolId::Address(pool_address));

        assert_eq!(pool.unwrap().get_address(), pool_address);
    }

    #[test]
    fn test_is_pool() {
        let mut market = Market::default();
        let pool_address = Address::random();
        let mock_pool = MockPool { address: pool_address, token0: Address::ZERO, token1: Address::ZERO };
        market.add_pool(mock_pool.clone());

        let is_pool = market.is_pool(&PoolId::Address(pool_address));

        assert!(is_pool);
    }

    #[test]
    fn test_is_pool_not_found() {
        let market = Market::<LoomDataTypesEthereum>::default();
        let pool_address = Address::random();

        let is_pool = market.is_pool(&PoolId::Address(pool_address));

        assert!(!is_pool);
    }

    #[test]
    fn test_set_pool_disabled() {
        let mut market = Market::default();
        let pool_address = Address::random();
        let token0 = Address::random();
        let token1 = Address::random();
        let mock_pool = MockPool { address: pool_address, token0, token1 };
        market.add_pool(mock_pool.clone());

        assert!(!market.is_pool_disabled(&PoolId::Address(pool_address)));
        assert_eq!(market.get_token_token_pools(&token0, &token1).unwrap().len(), 1);

        // toggle not ok
        market.set_pool_disabled(PoolId::Address(pool_address), true);
        assert!(market.is_pool_disabled(&PoolId::Address(pool_address)));
        assert_eq!(market.get_token_token_pools(&token0, &token1).unwrap().len(), 1);

        // toggle back
        market.set_pool_disabled(PoolId::Address(pool_address), false);
        assert!(!market.is_pool_disabled(&PoolId::Address(pool_address)));
        assert_eq!(market.get_token_token_pools(&token0, &token1).unwrap().len(), 1);
    }

    #[test]
    fn test_get_token_token_pools() {
        let mut market = Market::default();
        let pool_address = Address::random();
        let token0 = Address::random();
        let token1 = Address::random();
        let mock_pool = MockPool { address: pool_address, token0, token1 };
        market.add_pool(mock_pool);

        let pools = market.get_token_token_pools(&token0, &token1);

        assert_eq!(pools.unwrap().get(0).unwrap(), &PoolId::Address(pool_address));
    }

    #[test]
    fn test_get_token_tokens() {
        let mut market = Market::default();
        let pool_address = Address::random();
        let token0 = Address::random();
        let token1 = Address::random();
        let mock_pool = MockPool { address: pool_address, token0, token1 };
        market.add_pool(mock_pool);

        let tokens = market.get_token_tokens(&token0);

        assert_eq!(tokens.unwrap().get(0).unwrap(), &token1);
    }

    #[test]
    fn test_get_token_pools() {
        let mut market = Market::default();
        let pool_address = Address::random();
        let token0 = Address::random();
        let token1 = Address::random();
        let mock_pool = MockPool { address: pool_address, token0, token1 };
        market.add_pool(mock_pool);

        let pools = market.get_token_pools(&token0);

        assert_eq!(pools.unwrap().get(0).unwrap(), &PoolId::Address(pool_address));
    }

    #[test]
    fn test_build_swap_path_vec_two_hops() -> Result<()> {
        let mut market = Market::default();

        // Add basic token for start/end
        let weth_token = Token::new_with_data(TokenAddressEth::WETH, Some("WETH".to_string()), None, Some(18), true, false);
        market.add_token(weth_token);

        // Swap pool: token weth -> token1
        let pool_address1 = Address::random();
        let token1 = Address::random();
        let mock_pool1 = PoolWrapper::new(Arc::new(MockPool { address: pool_address1, token0: TokenAddressEth::WETH, token1 }));
        market.add_pool(mock_pool1.clone());

        // Swap pool: token weth -> token1
        let pool_address2 = Address::random();
        let mock_pool2 = PoolWrapper::new(Arc::new(MockPool { address: pool_address2, token0: TokenAddressEth::WETH, token1 }));
        market.add_pool(mock_pool2.clone());

        // Add test swap paths
        let mut directions = BTreeMap::new();
        directions.insert(mock_pool2.clone(), mock_pool2.get_swap_directions());
        let swap_paths = market.build_swap_path_vec(&directions)?;

        // verify that we have to paths, with 2 pools and 3 tokens
        assert_eq!(swap_paths.len(), 2);
        assert_eq!(swap_paths.get(0).unwrap().pool_count(), 2);
        assert_eq!(swap_paths.get(0).unwrap().tokens_count(), 3);
        assert_eq!(swap_paths.get(1).unwrap().pool_count(), 2);
        assert_eq!(swap_paths.get(1).unwrap().tokens_count(), 3);

        // the order of the swap paths is not deterministic
        let (first_path, second_path) = if swap_paths.get(0).unwrap().pools.get(0).unwrap().get_address() == pool_address1 {
            (swap_paths.get(0).unwrap(), swap_paths.get(1).unwrap())
        } else {
            (swap_paths.get(1).unwrap(), swap_paths.get(0).unwrap())
        };

        // first path weth -> token1 -> -> weth
        let tokens = first_path.tokens.iter().map(|token| token.get_address()).collect::<Vec<Address>>();
        assert_eq!(tokens.get(0), Some(&TokenAddressEth::WETH));
        assert_eq!(tokens.get(1), Some(&token1));
        assert_eq!(tokens.get(2), Some(&TokenAddressEth::WETH));

        let pools = first_path.pools.iter().map(|pool| pool.get_address()).collect::<Vec<Address>>();
        assert_eq!(pools.get(0), Some(&pool_address1));
        assert_eq!(pools.get(1), Some(&pool_address2));

        // other way around
        let tokens = second_path.tokens.iter().map(|token| token.get_address()).collect::<Vec<Address>>();
        assert_eq!(tokens.get(0), Some(&TokenAddressEth::WETH));
        assert_eq!(tokens.get(1), Some(&token1));
        assert_eq!(tokens.get(2), Some(&TokenAddressEth::WETH));

        let pools = second_path.pools.iter().map(|pool| pool.get_address()).collect::<Vec<Address>>();
        assert_eq!(pools.get(0), Some(&pool_address2));
        assert_eq!(pools.get(1), Some(&pool_address1));

        Ok(())
    }

    #[test]
    fn test_build_swap_path_vec_three_hops() -> Result<()> {
        let mut market = Market::default();

        // Add basic token for start/end
        let weth_token = Token::new_with_data(TokenAddressEth::WETH, Some("WETH".to_string()), None, Some(18), true, false);
        market.add_token(weth_token);

        // tokens
        let token1 = Address::random();
        let token2 = Address::random();

        // Swap pool: weth -> token1
        let pool_address1 = Address::random();
        let mock_pool = PoolWrapper::new(Arc::new(MockPool { address: pool_address1, token0: token1, token1: TokenAddressEth::WETH }));
        market.add_pool(mock_pool);

        // Swap pool: token1 -> token2
        let pool_address2 = Address::random();
        let mock_pool2 = PoolWrapper::new(Arc::new(MockPool { address: pool_address2, token0: token1, token1: token2 }));
        market.add_pool(mock_pool2);

        // Swap pool: token2 -> weth
        let pool_address3 = Address::random();
        let mock_pool3 = PoolWrapper::new(Arc::new(MockPool { address: pool_address3, token0: token2, token1: TokenAddressEth::WETH }));
        market.add_pool(mock_pool3.clone());

        // under test
        let mut directions = BTreeMap::new();
        directions.insert(mock_pool3.clone(), mock_pool3.get_swap_directions());
        let swap_paths = market.build_swap_path_vec(&directions)?;

        // verify that we have to paths, with 3 pools and 4 tokens
        assert_eq!(swap_paths.len(), 2);
        assert_eq!(swap_paths.get(0).unwrap().pool_count(), 3);
        assert_eq!(swap_paths.get(0).unwrap().tokens_count(), 4);
        assert_eq!(swap_paths.get(1).unwrap().pool_count(), 3);
        assert_eq!(swap_paths.get(1).unwrap().tokens_count(), 4);

        // the order of the swap paths is not deterministic
        let (first_path, second_path) = if swap_paths.get(0).unwrap().tokens.get(1).unwrap().get_address() == token1 {
            (swap_paths.get(0).unwrap(), swap_paths.get(1).unwrap())
        } else {
            (swap_paths.get(1).unwrap(), swap_paths.get(0).unwrap())
        };

        // first path weth -> token1 -> token2 -> weth
        let tokens = first_path.tokens.iter().map(|token| token.get_address()).collect::<Vec<Address>>();
        assert_eq!(tokens.get(0), Some(&TokenAddressEth::WETH));
        assert_eq!(tokens.get(1), Some(&token1));
        assert_eq!(tokens.get(2), Some(&token2));
        assert_eq!(tokens.get(3), Some(&TokenAddressEth::WETH));

        let pools = first_path.pools.iter().map(|pool| pool.get_address()).collect::<Vec<Address>>();
        assert_eq!(pools.get(0), Some(&pool_address1));
        assert_eq!(pools.get(1), Some(&pool_address2));
        assert_eq!(pools.get(2), Some(&pool_address3));

        // other way around
        let tokens = second_path.tokens.iter().map(|token| token.get_address()).collect::<Vec<Address>>();
        assert_eq!(tokens.get(0), Some(&TokenAddressEth::WETH));
        assert_eq!(tokens.get(1), Some(&token2));
        assert_eq!(tokens.get(2), Some(&token1));
        assert_eq!(tokens.get(3), Some(&TokenAddressEth::WETH));

        let pools = second_path.pools.iter().map(|pool| pool.get_address()).collect::<Vec<Address>>();
        assert_eq!(pools.get(0), Some(&pool_address3));
        assert_eq!(pools.get(1), Some(&pool_address2));
        assert_eq!(pools.get(2), Some(&pool_address1));

        Ok(())
    }
}
