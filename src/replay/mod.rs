// 回放解析层：单场解析（parser）、批量扫描（scanner）、战斗事件解码与射击推断（combat）、
// 客户端位置滤波器移植（filter，渲染层锚点，逆向文档 §七）、玩家开局配置与弹种映射（loadout）。
pub mod parser;
pub mod scanner;
pub mod combat;
pub mod filter;
pub mod loadout;
