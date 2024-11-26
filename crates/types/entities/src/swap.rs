use std::fmt::{Display, Formatter};
use std::sync::Arc;

use crate::{SwapAmountType, SwapLine, SwapStep, Token};
use alloy_primitives::U256;
use loom_types_blockchain::{LoomDataTypes, LoomDataTypesEthereum};

#[derive(Clone, Debug)]
pub enum Swap<LDT: LoomDataTypes = LoomDataTypesEthereum> {
    None,
    ExchangeSwapLine(SwapLine<LDT>),
    BackrunSwapSteps((SwapStep<LDT>, SwapStep<LDT>)),
    BackrunSwapLine(SwapLine<LDT>),
    Multiple(Vec<Swap<LDT>>),
}

impl<LDT: LoomDataTypes> Display for Swap<LDT> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Swap::ExchangeSwapLine(path) => write!(f, "{path}"),
            Swap::BackrunSwapLine(path) => write!(f, "{path}"),
            Swap::BackrunSwapSteps((sp0, sp1)) => write!(f, "{sp0} {sp1}"),
            Swap::Multiple(_) => write!(f, "MULTIPLE_SWAP"),
            Swap::None => write!(f, "UNKNOWN_SWAP_TYPE"),
        }
    }
}

impl<LDT: LoomDataTypes> Swap<LDT> {
    pub fn to_swap_steps(self: &Swap<LDT>, multicaller: LDT::Address) -> Option<(SwapStep<LDT>, SwapStep<LDT>)> {
        match self {
            Swap::BackrunSwapLine(swap_line) => {
                let mut sp0: Option<SwapLine<LDT>> = None;
                let mut sp1: Option<SwapLine<LDT>> = None;

                for i in 1..swap_line.path.pool_count() {
                    let (flash_path, inside_path) = swap_line.split(i).unwrap();
                    if flash_path.can_flash_swap() || inside_path.can_flash_swap() {
                        sp0 = Some(flash_path);
                        sp1 = Some(inside_path);
                        break;
                    }
                }

                if sp0.is_none() || sp1.is_none() {
                    let (flash_path, inside_path) = swap_line.split(1).unwrap();
                    sp0 = Some(flash_path);
                    sp1 = Some(inside_path);
                }

                let mut step_0 = SwapStep::new(multicaller);
                step_0.add(sp0.unwrap());

                let mut step_1 = SwapStep::new(multicaller);
                let mut sp1 = sp1.unwrap();
                sp1.amount_in = SwapAmountType::Balance(multicaller);
                step_1.add(sp1);

                Some((step_0, step_1))
            }
            Swap::BackrunSwapSteps((sp0, sp1)) => Some((sp0.clone(), sp1.clone())),
            _ => None,
        }
    }

    pub fn abs_profit(&self) -> U256 {
        match self {
            Swap::BackrunSwapLine(path) => path.abs_profit(),
            Swap::BackrunSwapSteps((sp0, sp1)) => SwapStep::abs_profit(sp0, sp1),
            Swap::Multiple(swap_vec) => swap_vec.iter().map(|x| x.abs_profit()).sum(),
            Swap::None => U256::ZERO,
            Swap::ExchangeSwapLine(_) => U256::ZERO,
        }
    }

    pub fn pre_estimate_gas(&self) -> u64 {
        match self {
            Swap::ExchangeSwapLine(path) => path.gas_used.unwrap_or_default(),
            Swap::BackrunSwapLine(path) => path.gas_used.unwrap_or_default(),
            Swap::BackrunSwapSteps((sp0, sp1)) => {
                sp0.swap_line_vec().iter().map(|i| i.gas_used.unwrap_or_default()).sum::<u64>()
                    + sp1.swap_line_vec().iter().map(|i| i.gas_used.unwrap_or_default()).sum::<u64>()
            }
            Swap::Multiple(swap_vec) => swap_vec.iter().map(|x| x.pre_estimate_gas()).sum(),
            Swap::None => 0,
        }
    }

    pub fn abs_profit_eth(&self) -> U256 {
        match self {
            Swap::ExchangeSwapLine(_) => U256::ZERO,
            Swap::BackrunSwapLine(path) => path.abs_profit_eth(),
            Swap::BackrunSwapSteps((sp0, sp1)) => SwapStep::abs_profit_eth(sp0, sp1),
            Swap::Multiple(swap_vec) => swap_vec.iter().map(|x| x.abs_profit_eth()).sum(),
            Swap::None => U256::ZERO,
        }
    }

    pub fn get_first_token(&self) -> Option<&Arc<Token<LDT>>> {
        match self {
            Swap::ExchangeSwapLine(swap_path) => swap_path.get_first_token(),
            Swap::BackrunSwapLine(swap_path) => swap_path.get_first_token(),
            Swap::BackrunSwapSteps((sp0, _sp1)) => sp0.get_first_token(),
            Swap::Multiple(_) => None,
            Swap::None => None,
        }
    }

    pub fn get_pool_address_vec(&self) -> Vec<LDT::Address> {
        match self {
            Swap::ExchangeSwapLine(swap_line) => swap_line.pools().iter().map(|item| item.get_address()).collect(),
            Swap::BackrunSwapLine(swap_line) => swap_line.pools().iter().map(|item| item.get_address()).collect(),
            Swap::BackrunSwapSteps((sp0, _sp1)) => {
                sp0.swap_line_vec().iter().flat_map(|item| item.pools().iter().map(|p| p.get_address()).collect::<Vec<_>>()).collect()
            }
            Swap::Multiple(swap_vec) => swap_vec.iter().flat_map(|x| x.get_pool_address_vec()).collect(),
            Swap::None => Vec::new(),
        }
    }
}
