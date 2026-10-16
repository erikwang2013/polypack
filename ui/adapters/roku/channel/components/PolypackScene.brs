' 交互子集的原生逻辑（「核心不参与，仅 UI 子集 + 原生逻辑」）：共享焦点内核
' （@polypack/adapter-tv 经 adapter-native 再导出）语义的 BrightScript 移植——
' 显式邻接表（source/focusgraph.brs，由发射器从内核快照生成）+ 边界自环 + Back 焦点记忆 + OK 激活。
' 键名映射见 ui/adapters/roku/src/roku-keys.js（up/down/left/right → DPAD_*，OK/select → DPAD_CENTER，back → Back）。
' 键事件在 Scene 层消费（focused 节点的 onKeyEvent 缺省 false → 冒泡至此）：返回 true 即接管，
' SceneGraph 内建几何焦点移动不再参与——这是内核语义（显式图）可移植的机制前提。

sub init()
    m.graph = ppFocusGraph()
    m.current = m.graph.initial
    m.history = []
    m.clicks = 0
    applyFocus()
end sub

function onKeyEvent(key as String, press as Boolean) as Boolean
    if not press then return false
    if key = "OK" or key = "select" then
        activate()
        return true
    end if
    if key = "back" then
        back()
        return true
    end if
    if key = "up" or key = "down" or key = "left" or key = "right" then
        move(key)
        return true
    end if
    return false
end function

' OK：激活当前节点；计数语义与 adapter-tv 示例页一致（counter 递增 / reset 归零）。
sub activate()
    if m.current = "counter" then
        m.clicks = m.clicks + 1
        m.top.findNode("label_counter").text = "点击计数：" + m.clicks.ToStr()
    else if m.current = "reset" then
        m.clicks = 0
        m.top.findNode("label_counter").text = "点击计数：0"
    end if
end sub

' Back：焦点记忆回退（内核 back() 语义）；单层 demo 无历史 = 顶层，退出宿主语义留分层 Back（adapter-tv T-M4-03 口径）。
sub back()
    if m.history.Count() = 0 then return
    focus(m.history.Pop())
end sub

sub move(dir as String)
    focus(m.graph.edges[m.current][dir])
end sub

' 焦点变更入历史（内核 focus() 语义：仅变更时入栈）。
sub focus(id as String)
    if id = m.current then return
    m.history.Push(m.current)
    m.current = id
    applyFocus()
end sub

' 焦点环切换（ring_<id>.visible）+ SceneGraph 焦点对齐（node_<id>.setFocus：保持事件链有焦点节点）。
sub applyFocus()
    for each id in m.graph.order
        ring = m.top.findNode("ring_" + id)
        if ring <> invalid then ring.visible = (id = m.current)
    end for
    node = m.top.findNode("node_" + m.current)
    if node <> invalid then node.setFocus(true)
end sub
