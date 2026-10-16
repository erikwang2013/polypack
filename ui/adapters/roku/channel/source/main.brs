' T-M4-11 spike 频道入口（形态对齐 rokudev/hello-world 官方样例）：
' roSGScreen + PolypackScene（components/PolypackScene.xml 由发射器生成）。
sub Main()
    screen = CreateObject("roSGScreen")
    m.port = CreateObject("roMessagePort")
    screen.setMessagePort(m.port)
    scene = screen.CreateScene("PolypackScene")
    screen.show()
    while true
        msg = wait(0, m.port)
        if type(msg) = "roSGScreenEvent" and msg.isScreenClosed() then return
    end while
end sub
