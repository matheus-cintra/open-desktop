import importlib.util
import pathlib
import socket
import struct
import unittest
from unittest.mock import patch

path = pathlib.Path(__file__).resolve().parents[1] / 'packaging/activity/opendesk-activity.py'
spec = importlib.util.spec_from_file_location('activity', path)
activity = importlib.util.module_from_spec(spec)
spec.loader.exec_module(activity)

class ActivityTest(unittest.TestCase):
    def test_filter_and_new_gesture(self):
        g = activity.Gesture()
        self.assertFalse(g.feed(1,30,0,1))
        self.assertFalse(g.feed(1,30,2,1))
        self.assertTrue(g.feed(1,30,1,1))
        self.assertTrue(g.feed(1,272,1,1))
        self.assertFalse(g.feed(2,0,2,1))
        self.assertTrue(g.feed(2,0,10,1.01))
        self.assertFalse(g.feed(2,0,100,1.02))
        self.assertTrue(g.feed(2,0,12,1.3))
    def test_absolute_initial_position_is_not_activity(self):
        g=activity.Gesture()
        self.assertFalse(g.feed(3,53,9000,1))
        self.assertFalse(g.feed(3,53,9001,1.01))
        self.assertTrue(g.feed(3,53,9012,1.02))
    def test_virtual_devices_are_excluded(self):
        with patch.object(activity.os.path,'exists',return_value=True), patch.object(activity.os.path,'realpath',return_value='/sys/devices/virtual/input/input9'):
            self.assertFalse(activity.physical('/dev/input/event9'))
    def test_socket_requires_active_uid(self):
        class Client:
            def getsockopt(self,*args): return struct.pack('3i',10,1000,1000)
        self.assertFalse(activity.authorized(Client(),set()))
        self.assertTrue(activity.authorized(Client(),{1000}))
        self.assertFalse(activity.authorized(Client(),{1001}))
    def test_logind_fails_closed_for_locked_remote_and_inactive(self):
        for flags in ['Active=no\nRemote=no\nLockedHint=no', 'Active=yes\nRemote=yes\nLockedHint=no', 'Active=yes\nRemote=no\nLockedHint=yes']:
            with patch.object(activity.subprocess,'check_output',side_effect=['1 1000 user',flags+'\nUser=1000\nType=wayland\nClass=user\nSeat=seat0']):
                self.assertEqual(activity.active_uids(),set())
        with patch.object(activity.subprocess,'check_output',side_effect=['1 1000 user','Active=yes\nRemote=no\nLockedHint=no\nUser=1000\nType=wayland\nClass=user\nSeat=seat0']):
            self.assertEqual(activity.active_uids(),{1000})

class SeatsTest(unittest.TestCase):
    def test_contacts_and_tool_buttons_are_not_takeovers(self):
        gesture=activity.Gesture({53:0.1})
        self.assertFalse(gesture.feed(1,330,1,1))
        self.assertFalse(gesture.feed(3,53,1000,1))
        self.assertFalse(gesture.feed(3,53,1002,1.01))
        gesture.feed(3,57,-1,1.02)
        self.assertFalse(gesture.feed(3,53,50000,1.03))
    def test_sessions_remain_separated_by_seat(self):
        props='Active=yes\nRemote=no\nLockedHint=no\nType=wayland\nClass=user\n'
        with patch.object(activity.subprocess,'check_output',side_effect=['1 1000 a\n2 1001 b',props+'User=1000\nSeat=seat0',props+'User=1001\nSeat=seat1']):
            self.assertEqual(activity.active_sessions(),{'seat0':{1000},'seat1':{1001}})

if __name__=='__main__': unittest.main()
